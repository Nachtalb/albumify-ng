//! Group a flat list of pending media into Telegram-compatible albums.
//!
//! Telegram's `sendMediaGroup` accepts photos, videos and documents (plus
//! audio / livephoto, which we don't queue). Documents must travel alone
//! with other documents; photos and videos can share a single album. We
//! preserve the user's insertion order and start a new group whenever the
//! next item cannot legally join the current one, and we cap each group at
//! Telegram's 10-item limit.
//!
//! Animations don't have their own `InputMedia*` variant accepted by
//! `sendMediaGroup` (the API only takes audio/document/photo/video/livephoto),
//! so we re-package their `file_id` as an `InputMediaVideo` — they're stored
//! as MP4s server-side, the file_id resolves the same way.

use teloxide::types::{
    InputFile, InputMedia, InputMediaDocument, InputMediaPhoto, InputMediaVideo,
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
/// expects. Every variant reuses the original `file_id`; animations are
/// re-tagged as `InputMediaVideo` because `sendMediaGroup` has no animation
/// variant.
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
        PendingMedia::Animation(id) => {
            // Animations are MP4s under the hood. Reuse the file_id as a
            // video — sendMediaGroup accepts it and Telegram renders the
            // result identically (silent video).
            InputMedia::Video(InputMediaVideo::new(InputFile::file_id(id.into())))
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
