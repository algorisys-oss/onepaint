use onepaint_history::History;

#[test]
fn entry_cap_keeps_newest() {
    let mut h = History::new();
    for i in 0..225 {
        h.push(i, 1);
    }
    assert_eq!(h.undo_len(), 200);
    let mut h = History::new();
    h.set_max_entries(5);
    for i in 0..12 {
        h.push(i, 1);
    }
    assert_eq!(h.undo_len(), 5);
    assert_eq!(h.undo().unwrap(), vec![&11]);
    let mut h = History::new();
    h.set_max_entries(40);
    for i in 0..40 {
        h.push(i, 1);
    }
    h.set_max_entries(10);
    assert_eq!(h.undo_len(), 10);
    h.set_max_entries(0);
    assert_eq!(h.undo_len(), 1);
}

#[test]
fn byte_budget_and_memory_limited() {
    let mut h = History::new();
    h.set_budget(3);
    h.set_max_entries(1000);
    for i in 0..3 {
        h.push(i, 1);
    }
    assert_eq!(h.undo_len(), 3);
    assert!(!h.memory_limited());
    h.push(3, 1);
    h.push(4, 1);
    assert_eq!(h.undo_len(), 3);
    assert!(h.memory_limited());
}

#[test]
fn transaction_order() {
    let mut h = History::new();
    h.transact(|t| {
        t.push(1, 1);
        t.push(2, 1);
        t.push(3, 1);
    });
    assert_eq!(h.undo_len(), 1);
    assert_eq!(h.undo().unwrap(), vec![&3, &2, &1]);
    assert_eq!(h.redo().unwrap(), vec![&1, &2, &3]);
}

#[test]
fn push_clears_redo() {
    let mut h = History::new();
    h.push("a", 1);
    h.push("b", 1);
    h.undo();
    assert!(h.can_redo());
    h.push("c", 1);
    assert!(!h.can_redo());
    assert_eq!(h.bytes(), 2);
}

#[test]
fn top_mut_for_coalescing() {
    let mut h = History::new();
    h.push(1, 1);
    *h.top_mut().unwrap() = 5;
    assert_eq!(h.undo().unwrap(), vec![&5]);
}
