//! Feedback: each account's conversation with the operator, bounded in
//! what a user sends and in what a conversation keeps.

use portal_core::feedback::MessageBody;

use super::*;
use crate::{
    Conversation, FeedbackMessage, MAX_KEPT, MAX_SENT_PER_DAY, ReplyError, SendError, Signals,
    StoreError,
};

fn body(text: &str) -> MessageBody {
    MessageBody::parse(text).unwrap()
}

fn message(from_operator: bool, text: &str, sent: u64) -> FeedbackMessage {
    FeedbackMessage {
        from_operator,
        body: body(text),
        sent_at: at(sent),
    }
}

/// An account signed in with session `byte`, which lasts two days.
fn signed_in(store: &mut Store, address: &str, bits: u64, byte: u8) -> Account {
    let account = register(store, address, bits);
    store
        .start_session(&session(&account, byte, NOW, 2 * 86_400))
        .unwrap();
    account
}

#[test]
fn a_conversation_goes_both_ways_oldest_first() {
    let mut store = store();
    let account = signed_in(&mut store, "a@qq.com", 1, 1);
    let other = signed_in(&mut store, "b@qq.com", 2, 2);
    store
        .send_feedback(&digest(1), &body("链接打不开"), at(NOW + 1))
        .unwrap();
    reply(
        &mut store,
        &account.code,
        &body("请重新生成链接"),
        at(NOW + 2),
    )
    .unwrap();
    store
        .send_feedback(&digest(1), &body("好了，谢谢"), at(NOW + 3))
        .unwrap();
    let expected = [
        message(false, "链接打不开", NOW + 1),
        message(true, "请重新生成链接", NOW + 2),
        message(false, "好了，谢谢", NOW + 3),
    ];
    assert_eq!(
        store
            .feedback_for_session(&digest(1), at(NOW + 4))
            .unwrap()
            .unwrap(),
        expected
    );
    assert_eq!(
        store.conversation_with(&account.code).unwrap().unwrap(),
        expected
    );
    // Another account's conversation is its own, and empty.
    assert!(
        store
            .feedback_for_session(&digest(2), at(NOW + 4))
            .unwrap()
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .conversation_with(&other.code)
            .unwrap()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.conversation_with(&AccountCode::from_bits(9)).unwrap(),
        None
    );
}

#[test]
fn only_a_live_session_sends_and_only_to_an_account_the_operator_replies() {
    let mut store = store();
    signed_in(&mut store, "a@qq.com", 1, 1);
    for (session, now) in [(digest(9), NOW), (digest(1), NOW + 2 * 86_400)] {
        assert!(matches!(
            store.send_feedback(&session, &body("你好"), at(now)),
            Err(SendError::SignedOut)
        ));
    }
    assert!(matches!(
        reply(
            &mut store,
            &AccountCode::from_bits(9),
            &body("你好"),
            at(NOW)
        ),
        Err(ReplyError::UnknownAccount(_))
    ));
    assert!(
        store
            .feedback_for_session(&digest(1), at(NOW))
            .unwrap()
            .unwrap()
            .is_empty()
    );
    // An unknown or ended session reads no conversation at all.
    for (session, now) in [(digest(9), NOW), (digest(1), NOW + 2 * 86_400)] {
        assert_eq!(store.feedback_for_session(&session, at(now)).unwrap(), None);
    }
}

#[test]
fn a_user_sends_a_bounded_number_a_day_and_replies_do_not_count() {
    let mut store = store();
    let account = signed_in(&mut store, "a@qq.com", 1, 1);
    signed_in(&mut store, "b@qq.com", 2, 2);
    for n in 0..MAX_SENT_PER_DAY {
        store
            .send_feedback(
                &digest(1),
                &body(&format!("第 {n} 条")),
                at(NOW + u64::from(n)),
            )
            .unwrap();
        reply(
            &mut store,
            &account.code,
            &body("收到"),
            at(NOW + u64::from(n)),
        )
        .unwrap();
    }
    assert!(matches!(
        store.send_feedback(&digest(1), &body("再一条"), at(NOW + 100)),
        Err(SendError::TooMany)
    ));
    // Each account counts its own.
    store
        .send_feedback(&digest(2), &body("我也有问题"), at(NOW + 100))
        .unwrap();
    // A day after the first, one more fits; then the limit holds again.
    let a_day_on = NOW + 86_400;
    store
        .send_feedback(&digest(1), &body("一天后"), at(a_day_on))
        .unwrap();
    assert!(matches!(
        store.send_feedback(&digest(1), &body("又一条"), at(a_day_on)),
        Err(SendError::TooMany)
    ));
    assert_eq!(
        store
            .feedback_for_session(&digest(2), at(a_day_on))
            .unwrap()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_conversation_keeps_its_latest_messages() {
    let mut store = store();
    let account = signed_in(&mut store, "a@qq.com", 1, 1);
    let other = signed_in(&mut store, "b@qq.com", 2, 2);
    store
        .send_feedback(&digest(2), &body("别删我的"), at(NOW))
        .unwrap();
    let total = u64::from(MAX_KEPT) + 5;
    for n in 0..total {
        reply(
            &mut store,
            &account.code,
            &body(&format!("回复 {n}")),
            at(NOW + n),
        )
        .unwrap();
    }
    let kept = store.conversation_with(&account.code).unwrap().unwrap();
    assert_eq!(kept.len(), usize::try_from(MAX_KEPT).unwrap());
    assert_eq!(kept[0], message(true, "回复 5", NOW + 5));
    assert_eq!(
        kept.last().unwrap(),
        &message(true, &format!("回复 {}", total - 1), NOW + total - 1)
    );
    assert_eq!(
        store.conversation_with(&other.code).unwrap().unwrap().len(),
        1
    );
}

#[test]
fn a_reply_is_unread_until_its_owner_reads_the_conversation() {
    let mut store = store();
    let account = signed_in(&mut store, "a@qq.com", 1, 1);
    assert_eq!(store.signals(account.id).unwrap(), Signals::default());
    store
        .send_feedback(&digest(1), &body("在吗"), at(NOW + 1))
        .unwrap();
    // A user's own message is never unread.
    assert_eq!(store.signals(account.id).unwrap(), Signals::default());
    reply(&mut store, &account.code, &body("在"), at(NOW + 2)).unwrap();
    assert!(store.signals(account.id).unwrap().unread_reply);
    // The operator reading it marks nothing.
    store.conversation_with(&account.code).unwrap();
    assert!(store.signals(account.id).unwrap().unread_reply);
    store
        .feedback_for_session(&digest(1), at(NOW + 3))
        .unwrap()
        .unwrap();
    assert!(!store.signals(account.id).unwrap().unread_reply);
    let seen: Vec<Option<i64>> = store
        .conn
        .prepare("SELECT seen_at FROM feedback ORDER BY id")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(seen, [None, Some((NOW + 3).cast_signed())]);
}

#[test]
fn the_operator_sees_who_waits_for_a_reply_first() {
    let mut store = store();
    let operator = signed_in(&mut store, "op@qq.com", 1, 1);
    let answered = signed_in(&mut store, "a@qq.com", 2, 2);
    let early = signed_in(&mut store, "b@qq.com", 3, 3);
    let late = signed_in(&mut store, "c@qq.com", 4, 4);
    signed_in(&mut store, "quiet@qq.com", 5, 5);
    store.grant_operator(&operator.code, at(NOW)).unwrap();
    store
        .send_feedback(&digest(2), &body("问题一"), at(NOW + 1))
        .unwrap();
    store
        .send_feedback(&digest(3), &body("问题二"), at(NOW + 2))
        .unwrap();
    reply(&mut store, &answered.code, &body("答复"), at(NOW + 3)).unwrap();
    store
        .send_feedback(&digest(4), &body("问题三"), at(NOW + 4))
        .unwrap();
    let conversation =
        |account: &Account, address: &str, last, awaiting_reply, messages| Conversation {
            account: account.code,
            email: email(address),
            last_sent_at: at(last),
            awaiting_reply,
            messages,
        };
    assert_eq!(
        store.conversations().unwrap(),
        [
            conversation(&late, "c@qq.com", NOW + 4, true, 1),
            conversation(&early, "b@qq.com", NOW + 2, true, 1),
            conversation(&answered, "a@qq.com", NOW + 3, false, 2),
        ]
    );
    assert_eq!(
        store.signals(operator.id).unwrap(),
        Signals {
            operator: true,
            unread_reply: false,
            awaiting_reply: 2,
            payments_waiting: 0,
        }
    );
    // Only an operator is told what waits.
    assert_eq!(store.signals(early.id).unwrap(), Signals::default());
}

#[test]
fn an_account_takes_its_conversation_with_it() {
    let mut store = store();
    let deleted = signed_in(&mut store, "a@qq.com", 1, 1);
    let banned = signed_in(&mut store, "b@qq.com", 2, 2);
    let kept = signed_in(&mut store, "c@qq.com", 3, 3);
    for (byte, account) in [(1, &deleted), (2, &banned), (3, &kept)] {
        store
            .send_feedback(&digest(byte), &body("问题"), at(NOW + 1))
            .unwrap();
        reply(&mut store, &account.code, &body("答复"), at(NOW + 2)).unwrap();
    }
    store.delete_account(&deleted.code).unwrap();
    store.ban(&banned.code, &[], at(NOW + 3)).unwrap();
    let left: Vec<i64> = store
        .conn
        .prepare("SELECT DISTINCT account_id FROM feedback")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(store.conversations().unwrap().len(), 1);
    assert_eq!(store.conversations().unwrap()[0].account, kept.code);
}

#[test]
fn a_stored_message_is_checked_as_it_is_read() {
    let mut store = store();
    let account = signed_in(&mut store, "a@qq.com", 1, 1);
    store
        .conn
        .execute(
            "INSERT INTO feedback (account_id, from_operator, body, sent_at)
             VALUES (?1, 0, ?2, ?3)",
            rusqlite::params![account.id.0, "a\u{1b}[31mb", NOW.cast_signed()],
        )
        .unwrap();
    assert!(matches!(
        store.conversation_with(&account.code),
        Err(StoreError::Corrupt { .. })
    ));
    assert!(matches!(
        store.feedback_for_session(&digest(1), at(NOW)),
        Err(StoreError::Corrupt { .. })
    ));
    // A user's own message is never seen, and nothing else may say who
    // wrote a message.
    for refused in [
        "INSERT INTO feedback (account_id, from_operator, body, sent_at, seen_at)
         VALUES (1, 0, 'x', 0, 0)",
        "INSERT INTO feedback (account_id, from_operator, body, sent_at) VALUES (1, 2, 'x', 0)",
    ] {
        assert!(store.conn.execute(refused, []).is_err(), "{refused}");
    }
}

#[test]
fn a_broken_database_is_an_error_never_an_empty_answer() {
    fn sqlite<T: std::fmt::Debug>(result: Result<T, StoreError>) {
        assert!(matches!(result, Err(StoreError::Sqlite(_))), "{result:?}");
    }
    let mut store = store();
    let account = signed_in(&mut store, "a@qq.com", 1, 1);
    store
        .conn
        .execute_batch("DROP TABLE feedback; DROP TABLE operators;")
        .unwrap();
    sqlite(store.signals(account.id));
    sqlite(store.conversations());
    sqlite(store.conversation_with(&account.code));
    sqlite(store.feedback_for_session(&digest(1), at(NOW)));
    sqlite(store.operator_session(&digest(1), at(NOW)));
    sqlite(store.as_operator(&digest(1), at(NOW), |_| Ok::<(), StoreError>(())));
    sqlite(store.confirm_session(&digest(1), &session(&account, 2, NOW, 3600)));
    assert!(matches!(
        store.send_feedback(&digest(1), &body("在吗"), at(NOW)),
        Err(SendError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        reply(&mut store, &account.code, &body("在"), at(NOW)),
        Err(ReplyError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.grant_operator(&account.code, at(NOW)),
        Err(crate::OperatorError::Store(StoreError::Sqlite(_)))
    ));
    assert!(matches!(
        store.revoke_operator(&account.code),
        Err(crate::OperatorError::Store(StoreError::Sqlite(_)))
    ));
}
