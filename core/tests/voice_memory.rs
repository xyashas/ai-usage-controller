use aiuc_core::ask::{self, voice_intent, Action, Context, Intent};
use aiuc_core::model::Tool;
use aiuc_core::store::Store;
use aiuc_core::{inbox, memory};
use chrono::Utc;

#[test]
fn commands_about_the_last_answer() {
    assert_eq!(voice_intent("Paste that.", true), Intent::Paste { code_only: false });
    assert_eq!(voice_intent("okay paste the code in", true), Intent::Paste { code_only: true });
    assert_eq!(voice_intent("Copy it", true), Intent::Copy);
    assert_eq!(voice_intent("make it shorter", true), Intent::Redo("shorter".into()));
    assert_eq!(voice_intent("More casual please", true), Intent::Redo("more casual".into()));
    assert_eq!(voice_intent("translate it to Spanish", true), Intent::Redo("translate it to spanish".into()));
    assert_eq!(voice_intent("in French", true), Intent::Redo("translate it in french".into()));
    assert_eq!(voice_intent("Send it to Codex", true), Intent::Send(Tool::Codex));
    assert_eq!(voice_intent("open that in Claude", true), Intent::Send(Tool::ClaudeCode));
    assert_eq!(voice_intent("say that again", true), Intent::Repeat);
    assert_eq!(voice_intent("Never mind.", true), Intent::Stop);
    // Without a recent answer, these are ordinary questions.
    assert_eq!(voice_intent("make it shorter", false), Intent::Ask);
    assert_eq!(voice_intent("paste that", false), Intent::Ask);
    // Real questions stay questions.
    assert_eq!(voice_intent("how do I paste without formatting in Word?", true), Intent::Ask);
    assert_eq!(voice_intent("what's the capital of France", true), Intent::Ask);
    assert_eq!(voice_intent("send an email to Sam about the launch", true), Intent::Ask);
}

#[test]
fn remembering_and_saving() {
    assert_eq!(voice_intent("Remember that I use TypeScript", false), Intent::Remember("I use TypeScript".into()));
    assert_eq!(voice_intent("always answer in bullet points", false), Intent::Remember("always answer in bullet points".into()));
    assert_eq!(voice_intent("Always returns undefined, why?", false), Intent::Ask);
    assert_eq!(voice_intent("never mind the tests, what does this do", false), Intent::Ask);
    assert_eq!(voice_intent("I prefer tabs, how do I set that in Prettier?", false), Intent::Ask);
    assert_eq!(voice_intent("never use semicolons", false), Intent::Remember("never use semicolons".into()));
    assert_eq!(voice_intent("I prefer short answers", false), Intent::Remember("I prefer short answers".into()));
    assert_eq!(voice_intent("forget that I use Vim", false), Intent::Forget("i use vim".into()));
    assert_eq!(voice_intent("remember this", true), Intent::Save(String::new()));
    assert_eq!(voice_intent("Save this for later", false), Intent::Save(String::new()));
    assert_eq!(voice_intent("Make a note: Buy the Figma licence", false), Intent::Save("Buy the Figma licence".into()));
    assert_eq!(voice_intent("Remind me at 5pm to call Sam.", false), Intent::Save("Remind me at 5pm to call Sam".into()));
}

#[test]
fn preferences_are_tidied_deduped_and_forgettable() {
    let s = Store::open_in_memory().unwrap();
    let now = Utc::now();
    assert_eq!(memory::tidy("remember that i use typescript"), "I use typescript.");
    memory::add(&s, "i use typescript", now).unwrap();
    memory::add(&s, "I use TypeScript.", now).unwrap();
    memory::add(&s, "keep it casual", now).unwrap();
    let l = memory::list(&s);
    assert_eq!(l.len(), 2);
    assert_ne!(l[0].id, l[1].id);
    assert_eq!(memory::forget_matching(&s, "typescript").unwrap(), 1);
    assert_eq!(memory::list(&s)[0].text, "Keep it casual.");
    for i in 0..(memory::MAX + 5) { memory::add(&s, &format!("pref {i}"), now).unwrap(); }
    assert_eq!(memory::list(&s).len(), memory::MAX);
    memory::clear(&s).unwrap();
    assert!(memory::list(&s).is_empty());
}

#[test]
fn preferences_go_into_the_prompt() {
    let prefs = vec![memory::Pref { id: 1, text: "I use TypeScript.".into(), at: Utc::now() }];
    let ctx = Context { about_me: memory::prompt_block(&prefs), ..Default::default() };
    let p = ask::build_prompt("write a debounce", Action::Ask, &ctx, &[], None);
    assert!(p.contains("<about_me>") && p.contains("- I use TypeScript."));
    assert!(!ask::build_prompt("hi", Action::Ask, &Context::default(), &[], None).contains("about_me"));
    assert!(memory::prompt_block(&[]).is_none());
}

#[test]
fn inbox_capture_contract() {
    let s = Store::open_in_memory().unwrap();
    let i = inbox::capture(&s, "Buy the Figma licence", inbox::Source::Voice, Some("Chrome"), Utc::now()).unwrap();
    assert_eq!(i.app.as_deref(), Some("Chrome"));
    assert!(inbox::capture(&s, "  ", inbox::Source::Voice, None, Utc::now()).is_err());
}
