//! Group a flat list of pending media into Telegram-compatible albums.
//!
//! Telegram's `sendMediaGroup` is fussy about mixing kinds:
//! - photos and videos may share one album
//! - documents must be alone (only with other documents)
//! - audio must be alone (only with other audio)
//!
//! We preserve the user's insertion order and start a new group whenever the
//! next item cannot legally join the current one, and we cap each group at
//! Telegram's 10-item limit.
//!
//! Animations (GIFs) aren't supported by `sendMediaGroup` under any
//! `InputMedia*` variant when reusing the original file_id, so the bot
//! refuses them at intake (see handlers.rs).

use teloxide::types::{
    InputFile, InputMedia, InputMediaAudio, InputMediaDocument, InputMediaPhoto, InputMediaVideo,
};

use crate::state::PendingMedia;

/// Compatibility category for grouping. Each category is a closed island —
/// items only share an album with siblings of the same kind, except photos
/// and videos which both live in `Visual`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Visual,
    Document,
    Audio,
}

fn classify(item: &PendingMedia) -> Kind {
    match item {
        PendingMedia::Photo(_) | PendingMedia::Video(_) => Kind::Visual,
        PendingMedia::Document(_) => Kind::Document,
        PendingMedia::Audio(_) => Kind::Audio,
    }
}

/// Pure planner: split a flat buffer into Telegram-compatible groups, capped
/// at 10 items per group.
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
/// expects. Every variant reuses the original `file_id` — no downloads.
pub fn resolve_group(items: Vec<PendingMedia>) -> Vec<InputMedia> {
    items.into_iter().map(resolve_item).collect()
}

fn resolve_item(item: PendingMedia) -> InputMedia {
    match item {
        PendingMedia::Photo(id) => {
            InputMedia::Photo(InputMediaPhoto::new(InputFile::file_id(id.into())))
        }
        PendingMedia::Video(id) => {
            InputMedia::Video(InputMediaVideo::new(InputFile::file_id(id.into())))
        }
        PendingMedia::Document(id) => {
            InputMedia::Document(InputMediaDocument::new(InputFile::file_id(id.into())))
        }
        PendingMedia::Audio(id) => {
            InputMedia::Audio(InputMediaAudio::new(InputFile::file_id(id.into())))
        }
    }
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
                        PendingMedia::Audio(_) => "audio",
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn photos_and_videos_share_one_group() {
        let input = vec![
            PendingMedia::Photo("p1".into()),
            PendingMedia::Video("v1".into()),
            PendingMedia::Photo("p2".into()),
        ];
        let groups = plan_groups(input);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 3);
    }

    #[test]
    fn documents_split_from_visuals() {
        // photo, photo, doc, doc, video -> (p, p)(d, d)(v)
        let input = vec![
            PendingMedia::Photo("p1".into()),
            PendingMedia::Photo("p2".into()),
            PendingMedia::Document("d1".into()),
            PendingMedia::Document("d2".into()),
            PendingMedia::Video("v1".into()),
        ];
        let groups = plan_groups(input);
        assert_eq!(
            kinds(&groups),
            vec![vec!["photo", "photo"], vec!["doc", "doc"], vec!["video"],]
        );
    }

    #[test]
    fn audio_is_its_own_island() {
        // audio, audio, photo, audio -> (a, a)(p)(a)
        let input = vec![
            PendingMedia::Audio("a1".into()),
            PendingMedia::Audio("a2".into()),
            PendingMedia::Photo("p1".into()),
            PendingMedia::Audio("a3".into()),
        ];
        let groups = plan_groups(input);
        assert_eq!(
            kinds(&groups),
            vec![vec!["audio", "audio"], vec!["photo"], vec!["audio"]]
        );
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
