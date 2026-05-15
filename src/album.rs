//! Group a flat list of pending media into Telegram-compatible albums.
//!
//! Telegram's `sendMediaGroup` allows mixing Photo + Video in one album, but
//! Document and Animation each need their own homogeneous group. We preserve
//! the user's insertion order and start a new group whenever the next item
//! cannot legally join the current one.

use teloxide::types::{
    InputFile, InputMedia, InputMediaAnimation, InputMediaDocument, InputMediaPhoto,
    InputMediaVideo,
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

fn to_input_media(item: PendingMedia) -> InputMedia {
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
            InputMedia::Animation(InputMediaAnimation::new(InputFile::file_id(id.into())))
        }
    }
}

/// Split the buffer into a sequence of compatible groups, capped at 10 items
/// per group (Telegram's limit).
///
/// Each output `Vec<InputMedia>` is a single `sendMediaGroup` call.
pub fn build_groups(items: Vec<PendingMedia>) -> Vec<Vec<InputMedia>> {
    const MAX_GROUP: usize = 10;
    let mut groups: Vec<Vec<InputMedia>> = Vec::new();
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

        groups
            .last_mut()
            .expect("just pushed")
            .push(to_input_media(item));
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(groups: &[Vec<InputMedia>]) -> Vec<Vec<&'static str>> {
        groups
            .iter()
            .map(|g| {
                g.iter()
                    .map(|m| match m {
                        InputMedia::Photo(_) => "photo",
                        InputMedia::Video(_) => "video",
                        InputMedia::Document(_) => "doc",
                        InputMedia::Animation(_) => "anim",
                        InputMedia::Audio(_) => "audio",
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn example_from_spec_splits_around_documents() {
        // image, image, document, document, video, animation
        //   -> (image, image)(document, document)(video, animation)
        // Only documents are exclusive; photo/video/animation all share.
        let input = vec![
            PendingMedia::Photo("p1".into()),
            PendingMedia::Photo("p2".into()),
            PendingMedia::Document("d1".into()),
            PendingMedia::Document("d2".into()),
            PendingMedia::Video("v1".into()),
            PendingMedia::Animation("a1".into()),
        ];

        let groups = build_groups(input);
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
        let groups = build_groups(input);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 4);
    }

    #[test]
    fn group_caps_at_ten() {
        let input: Vec<_> = (0..23)
            .map(|i| PendingMedia::Photo(format!("p{i}")))
            .collect();
        let groups = build_groups(input);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].len(), 10);
        assert_eq!(groups[1].len(), 10);
        assert_eq!(groups[2].len(), 3);
    }

    #[test]
    fn empty_input_yields_no_groups() {
        assert!(build_groups(vec![]).is_empty());
    }

    #[test]
    fn alternating_documents_stay_separate_from_photos() {
        let input = vec![
            PendingMedia::Document("d1".into()),
            PendingMedia::Photo("p1".into()),
            PendingMedia::Document("d2".into()),
        ];
        let groups = build_groups(input);
        assert_eq!(
            kinds(&groups),
            vec![vec!["doc"], vec!["photo"], vec!["doc"]]
        );
    }
}
