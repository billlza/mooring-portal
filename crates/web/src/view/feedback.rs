//! A conversation with the operator, as its pages and the chat script
//! show it.

use std::collections::BTreeMap;

use portal_store::{FeedbackMessage, Translation};
use serde::Serialize;

use crate::format::beijing;

/// Who reads a conversation: their own messages stand on the other side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reader {
    /// The account's owner.
    Owner,
    /// The operator.
    Operator,
}

/// A message of a conversation. The chat script reads it as the store
/// numbers it (`pages::chat`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct MessageView {
    /// Its number: every later message of the conversation has a larger
    /// one.
    pub(crate) id: u64,
    /// Whether the operator wrote it.
    pub(crate) from_operator: bool,
    /// Whether the reader wrote it.
    pub(crate) mine: bool,
    /// Who wrote it, as the reader calls them.
    pub(crate) who: String,
    /// When it was sent, in Beijing time.
    pub(crate) sent: String,
    /// Its text; the template keeps its line breaks.
    pub(crate) body: String,
    /// The message in the reader's language, where the host's translator
    /// wrote it in that one: the operator's page shows it under the
    /// message. The chat script is told of translations by themselves
    /// ([`TranslationView`]), as one may be made after its message was
    /// shown.
    #[serde(skip)]
    pub(crate) translation: Option<String>,
}

/// A translation of a message into the reader's language, as the chat
/// script is told of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct TranslationView {
    /// Its number: every translation made later has a larger one.
    pub(crate) id: u64,
    /// The number of the message it translates.
    pub(crate) message: u64,
    /// Its text, with its line breaks.
    pub(crate) body: String,
}

impl TranslationView {
    /// `translations`, in the order they were made.
    pub(crate) fn all(translations: &[Translation]) -> Vec<Self> {
        translations
            .iter()
            .map(|translation| Self {
                id: translation.id.get(),
                message: translation.message.get(),
                body: translation.text.as_str().to_owned(),
            })
            .collect()
    }
}

impl MessageView {
    /// `messages`, oldest first, as `reader` reads them, calling the
    /// account's owner `owner` and the operator `operator`.
    pub(crate) fn all(
        messages: &[FeedbackMessage],
        reader: Reader,
        owner: &str,
        operator: &str,
    ) -> Vec<Self> {
        messages
            .iter()
            .map(|message| Self {
                id: message.id.get(),
                from_operator: message.from_operator,
                mine: message.from_operator == (reader == Reader::Operator),
                who: if message.from_operator {
                    operator
                } else {
                    owner
                }
                .to_owned(),
                sent: beijing(message.sent_at),
                body: message.body.as_str().to_owned(),
                translation: None,
            })
            .collect()
    }

    /// `messages` with each of `translations` under its message. A
    /// translation of a message that is not among them is left out.
    pub(crate) fn translated(
        mut messages: Vec<Self>,
        translations: &[TranslationView],
    ) -> Vec<Self> {
        let by_message: BTreeMap<u64, &str> = translations
            .iter()
            .map(|translation| (translation.message, translation.body.as_str()))
            .collect();
        for message in &mut messages {
            message.translation = by_message.get(&message.id).map(|&text| text.to_owned());
        }
        messages
    }
}

#[cfg(test)]
mod tests {
    use portal_core::feedback::MessageBody;
    use portal_core::time::UnixTime;
    use portal_store::FeedbackId;

    use super::*;

    #[test]
    fn each_reader_finds_their_own_messages_on_their_side() {
        let message = |id, from_operator, secs| FeedbackMessage {
            id: FeedbackId::new(id),
            from_operator,
            body: MessageBody::parse("第一行\n第二行").unwrap(),
            sent_at: UnixTime::from_secs(secs).unwrap(),
        };
        // 2026-11-05T12:00:00Z is 20:00 in Beijing.
        let messages = [
            message(3, false, 1_793_880_000),
            message(8, true, 1_793_880_060),
        ];
        let owner = MessageView::all(&messages, Reader::Owner, "你", "运营者");
        let operator = MessageView::all(&messages, Reader::Operator, "abcd", "运营者");
        assert_eq!(
            owner,
            [
                MessageView {
                    id: 3,
                    from_operator: false,
                    mine: true,
                    who: "你".to_owned(),
                    sent: "2026-11-05 20:00".to_owned(),
                    body: "第一行\n第二行".to_owned(),
                    translation: None,
                },
                MessageView {
                    id: 8,
                    from_operator: true,
                    mine: false,
                    who: "运营者".to_owned(),
                    sent: "2026-11-05 20:01".to_owned(),
                    body: "第一行\n第二行".to_owned(),
                    translation: None,
                },
            ]
        );
        assert_eq!(
            operator
                .iter()
                .map(|message| (message.mine, message.who.as_str()))
                .collect::<Vec<_>>(),
            [(false, "abcd"), (true, "运营者")]
        );
    }
}
