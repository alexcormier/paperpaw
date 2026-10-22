use super::*;
use paperpaw_sdk::MemoryStorage;

fn texts(front: &str, back: &str) -> Texts {
    Texts {
        front: front.into(),
        back: back.into(),
        example: String::new(),
    }
}

const HUNDRED_DAYS: i64 = 100 * DAY;

#[test]
fn texts_with_tabs_and_line_breaks_come_back_as_they_were() {
    let card = Card {
        state: State {
            id: 3,
            due: 5,
            last: 4,
            stability: 317,
            difficulty: 528,
            reps: 1,
            lapses: 0,
        },
        front: "a\tb".into(),
        back: "line\nnext \\ end".into(),
        example: String::new(),
    };
    let line = card.line();
    assert_eq!(line.matches('\t').count(), 9, "{line}");
    assert_eq!(line.matches('\n').count(), 1);
    assert_eq!(Card::parse(line.trim_end_matches('\n')), Some(card));
    assert_eq!(Card::parse("not a card"), None);
}

#[test]
fn decks_and_cards_are_added_changed_and_deleted() {
    let storage = MemoryStorage::new();
    let words = add_deck(&storage, "  Words ").unwrap();
    assert_eq!(add_deck(&storage, " "), Err(Problem::Empty));
    assert_eq!(deck_named(&storage, "words"), Ok(words));
    let other = deck_named(&storage, "Phrases").unwrap();
    assert_ne!(other, words);
    assert_eq!(decks(&storage).len(), 2);

    let added = add(
        &storage,
        words,
        [
            texts("Hund", "dog"),
            texts(" ", "no front"),
            texts("Katze", "cat"),
        ],
    );
    assert_eq!(added, Ok(2));
    assert_eq!(add(&storage, 99, [texts("x", "y")]), Err(Problem::Missing));
    let (cards, total) = page(&storage, words, 0, 10);
    assert_eq!(total, 2);
    assert_eq!(cards[0].front, "Hund");
    assert_eq!((cards[0].state.id, cards[1].state.id), (1, 2));
    assert!(cards[0].state.is_new());

    edit(&storage, words, 2, &texts("Katze", "the cat")).unwrap();
    assert_eq!(page(&storage, words, 1, 1).0[0].back, "the cat");
    assert_eq!(
        edit(&storage, words, 9, &texts("a", "b")),
        Err(Problem::Missing)
    );
    delete(&storage, words, 1).unwrap();
    let (cards, total) = page(&storage, words, 0, 10);
    assert_eq!((total, cards[0].front.as_str()), (1, "Katze"));
    // A new card after a deleted one keeps a number of its own.
    add(&storage, words, [texts("Maus", "mouse")]).unwrap();
    assert_eq!(page(&storage, words, 1, 1).0[0].state.id, 3);
    assert_eq!(sizes(&storage)[&words], 2);

    rename_deck(&storage, other, "Sätze").unwrap();
    assert_eq!(decks(&storage)[1].name, "Sätze");
    delete_deck(&storage, other).unwrap();
    assert_eq!(decks(&storage).len(), 1);
    assert_eq!(delete_deck(&storage, other), Err(Problem::Missing));
}

#[test]
fn answers_are_kept_folded_and_counted() {
    let storage = MemoryStorage::new();
    let deck = add_deck(&storage, "Words").unwrap();
    add(
        &storage,
        deck,
        [texts("a", "1"), texts("b", "2"), texts("c", "3")],
    )
    .unwrap();
    let now = HUNDRED_DAYS;
    // All three are new: as many as may be shown.
    assert_eq!(due_count(&storage, now, 20), 3);
    assert_eq!(due_count(&storage, now, 2), 2);
    let queue = due(&storage, now, 20, 100, None);
    assert_eq!(queue.len(), 3);
    let first = card_at(&storage, deck, queue[0].offset).unwrap();
    assert_eq!(first.front, "a");

    // a: Good (due in three days); b: Again (due in ten minutes).
    let retention = Fx::hundredths(90);
    let good = queue[0].state.answered(Grade::Good, now, false, retention);
    record(&storage, deck, 0, &good).unwrap();
    let again = queue[1].state.answered(Grade::Again, now, false, retention);
    record(&storage, deck, 0, &again).unwrap();
    assert_eq!(good.due, now + 3 * DAY);
    // Due by a time is due before it: up to the quarter hour after a.
    let until = now + 3 * DAY + QUARTER;
    assert_eq!(again.due, now + fsrs::AGAIN_SECONDS);
    // Counted from the answers, before they are folded in.
    assert_eq!(due_count(&storage, now, 20), 1, "c, new");
    assert_eq!(due_count(&storage, now + 3600, 20), 2, "and b again");
    assert_eq!(due_count(&storage, until, 20), 3, "and a");

    fold(&storage).unwrap();
    assert!(!storage.exists(&answers()));
    assert_eq!(due_count(&storage, until, 20), 3);
    let (cards, _) = page(&storage, deck, 0, 3);
    assert_eq!(cards[0].state, good);
    assert_eq!(cards[1].state, again);
    assert!(cards[2].state.is_new());

    // Later: the reviews due first, then what is new.
    let later = due(&storage, until, 20, 100, Some(deck));
    let fronts: Vec<String> = later
        .iter()
        .map(|d| card_at(&storage, deck, d.offset).unwrap().front)
        .collect();
    assert_eq!(fronts, ["b", "a", "c"]);
    assert_eq!(due(&storage, until, 0, 1, None).len(), 1);
    assert!(due(&storage, until, 20, 100, Some(deck + 1)).is_empty());
    // b is due within the hour of now, a three days later.
    assert_eq!(next_due(&storage, now), Some(now + 3 * DAY));
}

#[test]
fn cards_are_added_from_a_file_a_line_at_a_time() {
    let storage = MemoryStorage::new();
    let deck = add_deck(&storage, "Words").unwrap();
    let file = [
        "Hund\tdog\tDer Hund bellt.",
        "",
        "Katze\tcat",
        "two\\nlines\tb",
    ]
    .join("\n");
    storage.write("import.tsv", file.as_bytes()).unwrap();
    assert_eq!(add_from(&storage, deck, "import.tsv"), Ok(3));
    let (cards, total) = page(&storage, deck, 0, 9);
    assert_eq!(total, 3);
    assert_eq!(cards[0].example, "Der Hund bellt.");
    assert_eq!(cards[2].front, "two\nlines");
    assert_eq!(sizes(&storage)[&deck], 3);
    assert_eq!(add_from(&storage, 7, "import.tsv"), Err(Problem::Missing));
}

#[test]
fn a_deck_cut_off_while_written_is_put_right() {
    let storage = MemoryStorage::new();
    let deck = add_deck(&storage, "Words").unwrap();
    add(&storage, deck, [texts("a", "1")]).unwrap();
    storage
        .write(&deck_temporary(deck), b"paperpaw-cards 1\n1\t0")
        .unwrap();
    recover(&storage);
    assert!(!storage.exists(&deck_temporary(deck)));
    assert_eq!(page(&storage, deck, 0, 9).1, 1);
}

#[test]
fn new_cards_are_counted_by_day() {
    let storage = MemoryStorage::new();
    let now = HUNDRED_DAYS + 3600;
    // Two hours ahead of UTC: that day ends at 22:00 UTC.
    let day = Day::of(now, now + 7200);
    assert_eq!(day.end, HUNDRED_DAYS + DAY - 7200);
    assert_eq!(new_left(&storage, day, 20), 20);
    new_shown(&storage, day, now);
    new_shown(&storage, day, now);
    assert_eq!(new_left(&storage, day, 20), 18);
    let tomorrow = Day::of(now + DAY, now + DAY + 7200);
    assert_eq!(new_left(&storage, tomorrow, 20), 20, "another day");
    // The time last known stays for a device without a clock.
    assert_eq!(kept(&storage).clock, now);
}

#[test]
fn cuts_leave_nothing_lost_nor_overwritten() {
    let storage = MemoryStorage::new();
    let deck = add_deck(&storage, "Words").unwrap();
    // A last line cut off: the next card goes on a line of its own.
    storage
        .append(&deck_path(deck), b"7\t0\t0\t0\t0\t0\t0\tcut")
        .unwrap();
    add(&storage, deck, [texts("Hund", "dog")]).unwrap();
    let (cards, total) = page(&storage, deck, 0, 9);
    assert_eq!(total, 2);
    assert_eq!(
        (cards[0].front.as_str(), cards[1].front.as_str()),
        ("cut", "Hund")
    );
    assert_eq!(cards[1].state.id, 8);

    // Cards added while the power went (counted nowhere yet): counted at
    // the start.
    storage
        .write(&adding(), format!("{deck}\n").as_bytes())
        .unwrap();
    storage
        .append(&deck_path(deck), b"9\t0\t0\t0\t0\t0\t0\tKatze\tcat\t\n")
        .unwrap();
    // (The card put there by hand above was not counted either.)
    assert_eq!(sizes(&storage)[&deck], 1);
    recover(&storage);
    assert_eq!(sizes(&storage)[&deck], 3);
    assert!(!storage.exists(&adding()));

    // A line that is no card stays as it is when the deck is written anew.
    storage
        .append(&deck_path(deck), b"a later version's line\n")
        .unwrap();
    delete(&storage, deck, 8).unwrap();
    let file = String::from_utf8(storage.get(&deck_path(deck)).unwrap()).unwrap();
    assert!(file.contains("a later version's line\n"), "{file}");
    assert!(!file.contains("Hund"));

    // A deck's file left without its list: a new deck goes past it.
    storage
        .write(&deck_path(deck + 1), b"paperpaw-cards 1\n")
        .unwrap();
    let other = add_deck(&storage, "Phrases").unwrap();
    assert_eq!(other, deck + 2);

    // The list cut while written whole: the new one, where the old is gone.
    let list = path("decks.txt");
    let whole = storage.get(&list).unwrap();
    storage.remove(&list).unwrap();
    storage.write(&files::part_path(&list), &whole).unwrap();
    recover(&storage);
    assert_eq!(decks(&storage).len(), 2);
}

#[test]
fn words_looked_up_wait_and_become_cards_once() {
    let storage = MemoryStorage::new();
    queue_lookup(&storage, "Hund", "Der Hund bellt.").unwrap();
    queue_lookup(&storage, "hund", "Noch einmal.").unwrap();
    queue_lookup(&storage, " ", "").unwrap();
    // Counted as new while they wait.
    assert_eq!(due_count(&storage, HUNDRED_DAYS, 20), 2);
    assert_eq!(take_lookups(&storage, "Looked up"), Ok(1));
    queue_lookup(&storage, "HUND", "").unwrap();
    queue_lookup(&storage, "Katze", "").unwrap();
    assert_eq!(take_lookups(&storage, "Looked up"), Ok(1));
    let deck = decks(&storage)[0].id;
    let (cards, _) = page(&storage, deck, 0, 9);
    assert_eq!(cards[0].example, "Der Hund bellt.");
    assert_eq!(cards[1].front, "Katze");
    assert_eq!(due_count(&storage, HUNDRED_DAYS, 20), 2);
}
