//! Group a flat list of pending media into Telegram-compatible albums.
//!
//! Telegram's `sendMediaGroup` accepts photos, videos and documents (plus
//! audio / livephoto, which we don't queue). Documents must travel alone
//! with other documents; photos and videos can share a single album. We
//! preserve the user's insertion order and start a new group whenever the
//! next item cannot legally join the current one, and we cap each group at
//! Telegram's 10-item limit.
//!
//! Animations are a special case. `sendMediaGroup` does **not** accept
//! `InputMediaAnimation`, and the file_id Telegram hands us when an animation
//! is received is type-bound — referencing it as a video yields
//! `"Wrong file identifier/HTTP URL specified"`. To send animations inside
//! an album we download the bytes via `getFile` + `Download::download_file`
//! and re-upload them as a fresh multipart `InputMedia::Video`. Telegram
//! converts the silent MP4 into a normal video on its side and accepts it.

use anyhow::{Context, Result};
use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::types::{
    FileId, InputFile, InputMedia, InputMediaDocument, InputMediaPhoto, InputMediaVideo,
};

use crate::state::PendingMedia;

/// Compatibility category for grouping.
///
/// Documents must be alone with other documents. Everything else — photos,
/// videos, animations — can share a single album.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Visual,
    Document,
}

fn classify(item: &PendingMedia) -> Kind {
    match item {
        PendingMedia::Document(_) => Kind::Document,
        PendingMedia::Photo(_) | PendingMedia::Video(_) | PendingMedia::Animation(_) => {
            Kind::Visual
        }
    }
}

/// Pure planner: split a flat buffer into Telegram-compatible groups, capped
/// at 10 items per group.
///
/// Returns the items themselves (not `InputMedia`) so the planner stays sync
/// and unit-testable. The async conversion to `InputMedia` happens in
/// [`resolve_group`].
pub fn plan_groups(items: Vec<PendingMedia>) -> Vec<Vec<PendingMedia>> {
    const MAX_GROUP: usize = 10;
    let mut groups: Vec<Vec<PendingMedia>> = Vec::new();
    let mut current_kind: Option<Kind> = None;

    for item in items {
        let kind = classify(&item);
        let needs_new_group = match (current_kind, groups.last()) {
            (Some(k), Some(last)) => k != kind || last.len() >= MAX_GROUP,
            _ => true,
        };

        if needs_new_group {
            groups.push(Vec::with_capacity(MAX_GROUP.min(8)));
            current_kind = Some(kind);
        }

        groups.last_mut().expect("just pushed").push(item);
    }

    groups
}

/// Convert one planned group into the `Vec<InputMedia>` that `sendMediaGroup`
/// expects.
///
/// Photos, videos and documents reuse their original `file_id`. Animations
/// are resolved via `getFile` to a temporary URL because their file_ids
/// cannot be referenced as `InputMediaVideo`.
pub async fn resolve_group(bot: &Bot, items: Vec<PendingMedia>) -> Result<Vec<InputMedia>> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        out.push(resolve_item(bot, item).await?);
    }
    Ok(out)
}

async fn resolve_item(bot: &Bot, item: PendingMedia) -> Result<InputMedia> {
    Ok(match item {
        PendingMedia::Photo(id) => {
            InputMedia::Photo(InputMediaPhoto::new(InputFile::file_id(id.into())))
        }
        PendingMedia::Video(id) => {
            InputMedia::Video(InputMediaVideo::new(InputFile::file_id(id.into())))
        }
        PendingMedia::Document(id) => {
            InputMedia::Document(InputMediaDocument::new(InputFile::file_id(id.into())))
        }
        PendingMedia::Animation(id) => {
            // Animation file_ids are type-bound and not reusable as a video.
            // We also can't pass the api.telegram.org/file/bot<TOKEN>/<path>
            // URL to sendMediaGroup — Telegram's server-side fetcher hits
            // WEBPAGE_CURL_FAILED because that URL is only accessible to
            // clients carrying the bot token, not to the public CDN puller.
            //
            // So: pull the bytes ourselves and re-upload as a fresh
            // multipart attachment. Telegram converts it to a regular
            // MP4 video on its side, which sendMediaGroup happily accepts.
            let file = bot
                .get_file(FileId(id.clone()))
                .await
                .with_context(|| format!("getFile failed for animation {id}"))?;
            let mut buf: Vec<u8> = Vec::with_capacity(file.size as usize);
            bot.download_file(&file.path, &mut buf)
                .await
                .with_context(|| format!("download failed for animation {id}"))?;
            let upload = InputFile::memory(buf).file_name("animation.mp4");
            InputMedia::Video(InputMediaVideo::new(upload))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(groups: &[Vec<PendingMedia>]) -> Vec<Vec<&'static str>> {
        groups
            .iter()
            .map(|g| {
                g.iter()
                    .map(|m| match m {
                        PendingMedia::Photo(_) => "photo",
                        PendingMedia::Video(_) => "video",
                        PendingMedia::Document(_) => "doc",
                        PendingMedia::Animation(_) => "anim",
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn example_from_spec_splits_around_documents() {
        // image, image, document, document, video, animation
        //   -> (image, image)(document, document)(video, animation)
        // Only documents are exclusive; photo/video/animation all share
        // at the planning layer (animation is resolved to a video URL at
        // send time, see resolve_item).
        let input = vec![
            PendingMedia::Photo("p1".into()),
            PendingMedia::Photo("p2".into()),
            PendingMedia::Document("d1".into()),
            PendingMedia::Document("d2".into()),
            PendingMedia::Video("v1".into()),
            PendingMedia::Animation("a1".into()),
        ];

        let groups = plan_groups(input);
        assert_eq!(
            kinds(&groups),
            vec![
                vec!["photo", "photo"],
                vec!["doc", "doc"],
                vec!["video", "anim"],
            ]
        );
    }

    #[test]
    fn photo_video_and_animation_share_one_group() {
        let input = vec![
            PendingMedia::Photo("p1".into()),
            PendingMedia::Video("v1".into()),
            PendingMedia::Animation("a1".into()),
            PendingMedia::Photo("p2".into()),
        ];
        let groups = plan_groups(input);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 4);
    }

    #[test]
    fn group_caps_at_ten() {
        let input: Vec<_> = (0..23)
            .map(|i| PendingMedia::Photo(format!("p{i}")))
            .collect();
        let groups = plan_groups(input);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].len(), 10);
        assert_eq!(groups[1].len(), 10);
        assert_eq!(groups[2].len(), 3);
    }

    #[test]
    fn empty_input_yields_no_groups() {
        assert!(plan_groups(vec![]).is_empty());
    }

    #[test]
    fn alternating_documents_stay_separate_from_photos() {
        let input = vec![
            PendingMedia::Document("d1".into()),
            PendingMedia::Photo("p1".into()),
            PendingMedia::Document("d2".into()),
        ];
        let groups = plan_groups(input);
        assert_eq!(
            kinds(&groups),
            vec![vec!["doc"], vec!["photo"], vec!["doc"]]
        );
    }
}
