use aiuc_core::inbox::{self, Source};
use aiuc_core::store::Store;
use chrono::{Duration, Local, TimeZone, Timelike, Utc};

#[test]
fn capture_list_done_update_remove_and_clear_done() {
    let store = Store::open_in_memory().unwrap();
    let t0 = Utc::now() - Duration::minutes(10);
    let t1 = Utc::now() - Duration::minutes(5);
    let t2 = Utc::now();

    // Empty text rejected
    assert!(inbox::capture(&store, "   ", Source::Voice, None, t0).is_err());

    let a = inbox::capture(&store, "  Check Stripe webhook retries ", Source::Voice, Some("Chrome"), t0).unwrap();
    let b = inbox::capture(&store, "Add dark mode contrast check", Source::Ask, Some("VS Code"), t1).unwrap();
    let c = inbox::capture(&store, "Follow up on PR #5", Source::Selection, None, t2).unwrap();

    assert_eq!(a.text, "Check Stripe webhook retries");
    assert_eq!(a.app.as_deref(), Some("Chrome"));
    assert_eq!(a.source, Source::Voice);
    assert!(!a.done);

    // list() returns newest first
    let all = inbox::list(&store);
    assert_eq!(all.iter().map(|i| i.id).collect::<Vec<_>>(), vec![c.id, b.id, a.id]);

    // update_text
    let updated = inbox::update_text(&store, a.id, "Check Stripe webhook retries on staging")
        .unwrap()
        .expect("item should exist");
    assert_eq!(updated.text, "Check Stripe webhook retries on staging");
    assert!(inbox::update_text(&store, a.id, "  ").is_err());
    assert!(inbox::update_text(&store, 999_999, "nope").unwrap().is_none());

    // set_done + clear_done
    let done_a = inbox::set_done(&store, a.id, true).unwrap().unwrap();
    assert!(done_a.done);
    inbox::set_done(&store, b.id, true).unwrap().unwrap();
    assert_eq!(inbox::clear_done(&store).unwrap(), 2);
    assert_eq!(inbox::list(&store).len(), 1);
    assert_eq!(inbox::list(&store)[0].id, c.id);

    // remove
    assert!(inbox::remove(&store, c.id).unwrap());
    assert!(!inbox::remove(&store, c.id).unwrap());
    assert!(inbox::list(&store).is_empty());
}

#[test]
fn parse_when_and_set_remind_at() {
    let store = Store::open_in_memory().unwrap();
    let morning = Local
        .with_ymd_and_hms(2026, 11, 27, 10, 0, 0)
        .earliest()
        .unwrap()
        .with_timezone(&Utc);

    let later = inbox::parse_when("later today", morning).expect("later today");
    assert_eq!(later.with_timezone(&Local).hour(), 17);

    let tomorrow = inbox::parse_when("tomorrow 9am", morning).expect("tomorrow 9am");
    assert_eq!(tomorrow.with_timezone(&Local).hour(), 9);
    assert!(tomorrow > morning);

    let in_30 = inbox::parse_when("in 30 minutes", morning).expect("in 30 minutes");
    assert_eq!(in_30, morning + Duration::minutes(30));

    let item = inbox::capture(&store, "Ship release notes", Source::Voice, None, morning).unwrap();
    let updated = inbox::set_remind_at(&store, item.id, Some(later)).unwrap().unwrap();
    assert_eq!(updated.remind_at, Some(later));
}
