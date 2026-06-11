//! Invariant proof (design §2): under heavy concurrent contention the ledger
//! must NEVER let committed spend exceed the budget, and must grant exactly the
//! number of reservations the budget allows.

use std::sync::Arc;
use std::thread;

use gateway_spine::{BudgetLedger, Usd};

#[test]
fn never_overspends_under_concurrency() {
    let ledger = Arc::new(BudgetLedger::new());
    // $1.00 budget; each call costs exactly $0.10 → at most 10 may succeed.
    ledger.set_budget("k", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);
    let cost = Usd::from_dollars_f64(0.10);

    let mut handles = Vec::new();
    for _ in 0..200 {
        let l = Arc::clone(&ledger);
        handles.push(thread::spawn(move || match l.reserve("k", cost) {
            Ok(r) => {
                // Always commit the full estimate (worst case for overspend).
                l.commit(r, cost).unwrap();
                true
            }
            Err(_) => false,
        }));
    }

    let successes = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|&ok| ok)
        .count();

    assert_eq!(
        successes, 10,
        "exactly 10 reservations of $0.10 fit in $1.00"
    );
    assert_eq!(
        ledger.spent("k"),
        Usd::from_dollars_f64(1.0),
        "never overspends"
    );
    assert_eq!(ledger.reserved("k"), Usd::ZERO, "no dangling reservations");
}
