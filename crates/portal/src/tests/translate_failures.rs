//! `translate` when the translator fails: a question that fails leaves
//! its message for the next run, a translator that cannot be reached or
//! keeps failing stops the run, a message it fails on every time does not
//! keep the newer ones waiting, and a run that cannot say what it did
//! fails.

use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portal_core::translation::{MAX_TRANSLATION_CHARS, PageLanguage};

use super::translate::{Reply, StandIn, fluent, none, ok, translations, waiting, write};
use super::{FailAfter, Fixture};
use crate::translate::{MAX_FAILURES, Translator, run};
use crate::{CommandError, TranslatorError};

#[test]
fn a_question_that_fails_leaves_its_message_for_the_next_run() {
    const NEITHER: &str = "it answered neither `ok` with what was asked for nor `none`";
    const WRONG_LENGTH: &str = "its answer is not as long as it says";
    for (bad, said) in [
        (Reply::from("failed\n"), NEITHER),
        (Reply::from(""), NEITHER),
        // A first line that does not end, or says no number of bytes, or
        // says it otherwise than in digits.
        (Reply::from("none"), NEITHER),
        (Reply::from("ok\nen\n"), NEITHER),
        (Reply::from("ok \nen"), NEITHER),
        (Reply::from("ok +2\nen"), NEITHER),
        (Reply::from("ok 2 \nen"), NEITHER),
        (Reply::from("ok 99999999999999999999999\nen"), NEITHER),
        // Nothing follows `none`.
        (Reply::from("none\nen"), NEITHER),
        // No language.
        (ok("zh-Hant"), NEITHER),
        // Cut short, as when the translator is stopped half-way, and
        // counted wrongly.
        (Reply::from("ok 5\nen"), WRONG_LENGTH),
        (Reply::from("ok 2\nen\n"), WRONG_LENGTH),
        (Reply::from("ok 0\nen"), WRONG_LENGTH),
        (
            Reply::Bytes(b"ok 2\n\xff\xfe".to_vec()),
            "its answer is not UTF-8 text",
        ),
        (
            Reply::Bytes(vec![b'w'; 9 * MAX_TRANSLATION_CHARS]),
            "its answer is too long",
        ),
    ] {
        let fixture = Fixture::new();
        let broken = Arc::new(AtomicBool::new(true));
        let still = Arc::clone(&broken);
        let bad = Mutex::new(Some(bad));
        let translator = StandIn::new(fixture.dir.path(), move |line, text| {
            if text == "second" && line == "detect" && still.load(Ordering::SeqCst) {
                bad.lock().unwrap().take().unwrap()
            } else {
                fluent(line, text)
            }
        });
        let (alice, session) = fixture.signed_in("alice@qq.com", 1);
        for text in ["first", "second", "third"] {
            write(&fixture, &session, text);
        }
        let args = ["translate", "--translator", translator.socket()];
        let failed = fixture.run(&args);
        assert_eq!(failed.code, ExitCode::FAILURE, "{said}");
        // The other messages are examined all the same.
        assert_eq!(
            failed.out,
            "examined 2 message(s) and wrote 2 translation(s)\n"
        );
        assert_eq!(
            failed.err,
            format!(
                "mooring-portal: the translator at {} left 1 message(s) for the next run: {said}\n",
                translator.socket()
            )
        );
        // Neither says anything of a message or of whose it is.
        for said in [&failed.out, &failed.err] {
            for trace in ["second", "alice", &alice.to_string()] {
                assert!(!said.contains(trace), "{said}");
            }
        }
        assert_eq!(waiting(&fixture), [2]);
        broken.store(false, Ordering::SeqCst);
        let mended = fixture.run(&args);
        assert_eq!(mended.code, ExitCode::SUCCESS, "{}", mended.err);
        assert_eq!(
            mended.out,
            "examined 1 message(s) and wrote 1 translation(s)\n"
        );
        assert_eq!(
            translations(&fixture, &alice, PageLanguage::Chinese),
            ["3: [en>zh] third", "1: [en>zh] first", "2: [en>zh] second"]
        );
    }
}

#[test]
fn a_translator_that_keeps_failing_is_not_asked_about_every_message() {
    let fixture = Fixture::new();
    let translator = StandIn::new(fixture.dir.path(), |_, _| Reply::from("failed\n"));
    let (_, session) = fixture.signed_in("alice@qq.com", 1);
    for text in ["one", "two", "three", "four", "five"] {
        write(&fixture, &session, text);
    }
    let failed = fixture.run(&["translate", "--translator", translator.socket()]);
    assert_eq!(failed.code, ExitCode::FAILURE);
    assert_eq!(
        failed.out,
        "examined 0 message(s) and wrote 0 translation(s)\n"
    );
    assert_eq!(
        failed.err,
        format!(
            "mooring-portal: the translator at {} left {MAX_FAILURES} message(s) for the next \
             run: it answered neither `ok` with what was asked for nor `none`\n",
            translator.socket()
        )
    );
    assert_eq!(
        translator.asked(),
        ["detect|five", "detect|four", "detect|three"]
    );
    assert_eq!(waiting(&fixture), [1, 2, 3, 4, 5]);
}

#[test]
fn without_a_translator_the_run_stops_at_the_first_message() {
    let fixture = Fixture::new();
    let nowhere = fixture.dir.path().join("no-translator.sock");
    let args = ["translate", "--translator", nowhere.to_str().unwrap()];
    // With nothing to ask about, the translator is not looked for.
    let idle = fixture.run(&args);
    assert_eq!(idle.code, ExitCode::SUCCESS, "{}", idle.err);
    assert_eq!(idle.out, "no message waits for the translator\n");

    let (_, session) = fixture.signed_in("alice@qq.com", 1);
    for text in ["one", "two"] {
        write(&fixture, &session, text);
    }
    let failed = fixture.run(&args);
    assert_eq!(failed.code, ExitCode::FAILURE);
    assert_eq!(
        failed.out,
        "examined 0 message(s) and wrote 0 translation(s)\n"
    );
    let begins = format!(
        "mooring-portal: the translator at {} left 1 message(s) for the next run: it cannot be \
         reached: ",
        nowhere.display()
    );
    assert!(failed.err.starts_with(&begins), "{}", failed.err);
    assert_eq!(waiting(&fixture), [1, 2]);
    // A file that is no socket is no translator either.
    let file = fixture.file("plain", b"");
    let refused = fixture.run(&["translate", "--translator", file.to_str().unwrap()]);
    assert_eq!(refused.code, ExitCode::FAILURE);
    assert!(
        refused.err.contains("it cannot be reached"),
        "{}",
        refused.err
    );
}

#[test]
fn a_translator_that_says_nothing_is_given_up_on() {
    let fixture = Fixture::new();
    let translator = StandIn::new(fixture.dir.path(), |_, _| Reply::Silence);
    let (_, session) = fixture.signed_in("alice@qq.com", 1);
    write(&fixture, &session, "anyone there?");
    let mut out = Vec::new();
    let began = Instant::now();
    let failed = run(
        &mut fixture.store(),
        &fixture.clock,
        &Translator {
            socket: Path::new(translator.socket()),
            patience: Duration::from_millis(200),
        },
        &mut out,
    );
    assert!(began.elapsed() < Duration::from_secs(30));
    match failed {
        Err(CommandError::Translator {
            failed: 1,
            source: TranslatorError::Silent(_),
            ..
        }) => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "examined 0 message(s) and wrote 0 translation(s)\n"
    );
    assert_eq!(waiting(&fixture), [1]);
}

#[test]
fn a_message_that_keeps_failing_does_not_keep_newer_ones_waiting() {
    let fixture = Fixture::new();
    let translator = StandIn::new(fixture.dir.path(), |line, text| {
        if text.starts_with("bad") {
            Reply::from("failed\n")
        } else {
            fluent(line, text)
        }
    });
    let (alice, session) = fixture.signed_in("alice@qq.com", 1);
    for text in ["bad one", "bad two", "bad three", "old but good"] {
        write(&fixture, &session, text);
    }
    let args = ["translate", "--translator", translator.socket()];
    // The newest is examined, and whatever older one the translator
    // manages, before those it fails on stop the run.
    for (text, asked) in [("new", 7), ("newer", 12)] {
        write(&fixture, &session, text);
        let failed = fixture.run(&args);
        assert_eq!(failed.code, ExitCode::FAILURE, "{text}");
        assert!(
            failed
                .err
                .contains(&format!("left {MAX_FAILURES} message(s) for the next run")),
            "{}",
            failed.err
        );
        assert_eq!(waiting(&fixture), [1, 2, 3], "{text}");
        assert_eq!(translator.asked().len(), asked, "{text}");
    }
    assert_eq!(
        translator.asked()[..5],
        [
            "detect|new",
            "translate en zh|new",
            "detect|old but good",
            "translate en zh|old but good",
            "detect|bad three",
        ]
    );
    assert_eq!(
        translations(&fixture, &alice, PageLanguage::Chinese),
        [
            "5: [en>zh] new",
            "4: [en>zh] old but good",
            "6: [en>zh] newer"
        ]
    );
}

#[test]
fn a_translation_that_fails_leaves_its_message_and_one_not_made_is_left_out() {
    let fixture = Fixture::new();
    let broken = Arc::new(AtomicBool::new(true));
    let still = Arc::clone(&broken);
    // A translator with no model for Chinese, which fails on Japanese.
    let translator = StandIn::new(fixture.dir.path(), move |line, text| match line {
        "translate en zh" => none(),
        "translate ja en" if still.load(Ordering::SeqCst) => Reply::from("failed\n"),
        _ => fluent(line, text),
    });
    let (alice, session) = fixture.signed_in("alice@qq.com", 1);
    write(&fixture, &session, "こんにちは");
    write(&fixture, &session, "Hello");
    let args = ["translate", "--translator", translator.socket()];
    let failed = fixture.run(&args);
    assert_eq!(failed.code, ExitCode::FAILURE);
    // The English message is examined, with no Chinese to show.
    assert_eq!(
        failed.out,
        "examined 1 message(s) and wrote 0 translation(s)\n"
    );
    assert!(
        failed.err.ends_with(
            " left 1 message(s) for the next run: it answered neither `ok` with what was asked \
             for nor `none`\n"
        ),
        "{}",
        failed.err
    );
    // Nothing is kept of the Japanese one, not the language found: it is
    // asked about from the start.
    assert_eq!(waiting(&fixture), [1]);
    broken.store(false, Ordering::SeqCst);
    let mended = fixture.run(&args);
    assert_eq!(mended.code, ExitCode::SUCCESS, "{}", mended.err);
    assert_eq!(
        mended.out,
        "examined 1 message(s) and wrote 1 translation(s)\n"
    );
    assert_eq!(
        translator.asked(),
        [
            "detect|Hello",
            "translate en zh|Hello",
            "detect|こんにちは",
            "translate ja en|こんにちは",
            "detect|こんにちは",
            "translate ja en|こんにちは",
            "translate en zh|[ja>en] こんにちは",
        ]
    );
    assert_eq!(
        translations(&fixture, &alice, PageLanguage::English),
        ["1: [ja>en] こんにちは"]
    );
    assert_eq!(
        translations(&fixture, &alice, PageLanguage::Chinese),
        Vec::<String>::new()
    );
}

#[test]
fn a_run_that_cannot_say_what_it_did_fails() {
    const SAID: &str = "examined 1 message(s) and wrote 0 translation(s); 1 translation(s) could \
                        not be shown and were not kept\n";
    let run_with = |fixture: &Fixture, socket: &str, budget: usize| {
        let db = fixture.db();
        let line = [
            "mooring-portal",
            "--db",
            db.to_str().unwrap(),
            "translate",
            "--translator",
            socket,
        ];
        let mut err = Vec::new();
        let code = crate::run(line, &fixture.clock, &mut FailAfter(budget), &mut err);
        (code, String::from_utf8(err).unwrap())
    };
    // The output closes before the run says anything, in what it says of
    // the translations not kept, and at the end of the line.
    for budget in [0, 48, SAID.len() - 1] {
        let fixture = Fixture::new();
        let translator = StandIn::new(fixture.dir.path(), |line, text| match line {
            "translate en zh" => ok(" "),
            _ => fluent(line, text),
        });
        let (_, session) = fixture.signed_in("alice@qq.com", 1);
        write(&fixture, &session, "Hello");
        let (code, err) = run_with(&fixture, translator.socket(), budget);
        assert_eq!(code, ExitCode::FAILURE, "{budget}");
        assert_eq!(
            err, "mooring-portal: cannot write the output: pipe closed\n",
            "{budget}"
        );
        // What the translator said was recorded before.
        assert_eq!(waiting(&fixture), Vec::<u64>::new(), "{budget}");
    }
    // With nothing to say but that no message waits.
    let fixture = Fixture::new();
    let nowhere = fixture.dir.path().join("no-translator.sock");
    let (code, err) = run_with(&fixture, nowhere.to_str().unwrap(), 0);
    assert_eq!(code, ExitCode::FAILURE);
    assert_eq!(
        err,
        "mooring-portal: cannot write the output: pipe closed\n"
    );
}
