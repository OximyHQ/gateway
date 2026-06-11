//! The atomic budget ledger. Each key tracks `spent` and `reserved` under one
//! lock. `reserve` is FAIL-CLOSED: if the reservation would push
//! spent + reserved over the budget, it errors *before* any upstream call.
//! `commit` trues-up actual vs the estimate; `release` drops a reservation that
//! never billed. Unlimited budget (`None`) always reserves. In-memory for P1.1;
//! P1.6 swaps the backing store, P-later distributes it.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::SpineError;
use crate::money::Usd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReservationId(u64);

#[derive(Debug, Default)]
struct KeyBudget {
    budget: Option<Usd>,
    spent: Usd,
    reserved: Usd,
}

#[derive(Debug, Clone)]
struct Reservation {
    key_id: String,
    estimate: Usd,
}

#[derive(Default)]
struct Inner {
    budgets: HashMap<String, KeyBudget>,
    reservations: HashMap<u64, Reservation>,
    next_res: u64,
}

#[derive(Default)]
pub struct BudgetLedger {
    inner: Mutex<Inner>,
}

impl BudgetLedger {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed or restore a key's budget and prior spend.
    pub fn set_budget(&self, key_id: &str, budget: Option<Usd>, spent: Usd) {
        let mut g = self.inner.lock().unwrap();
        g.budgets.insert(
            key_id.to_string(),
            KeyBudget {
                budget,
                spent,
                reserved: Usd::ZERO,
            },
        );
    }

    /// FAIL-CLOSED reservation. Errors if the key is unknown or would overspend.
    pub fn reserve(&self, key_id: &str, estimate: Usd) -> Result<ReservationId, SpineError> {
        let mut g = self.inner.lock().unwrap();
        {
            let kb = g
                .budgets
                .get_mut(key_id)
                .ok_or_else(|| SpineError::NoSuchKey {
                    key_id: key_id.to_string(),
                })?;
            if let Some(budget) = kb.budget {
                let would = kb.spent + kb.reserved + estimate;
                if would > budget {
                    return Err(SpineError::budget_exceeded(key_id, would, budget));
                }
            }
            kb.reserved += estimate;
        }
        let id = g.next_res;
        g.next_res += 1;
        g.reservations.insert(
            id,
            Reservation {
                key_id: key_id.to_string(),
                estimate,
            },
        );
        Ok(ReservationId(id))
    }

    /// Commit a reservation with the ACTUAL cost (true-up). Reserved -= estimate,
    /// spent += actual.
    pub fn commit(&self, res: ReservationId, actual: Usd) -> Result<(), SpineError> {
        let mut g = self.inner.lock().unwrap();
        let r = g
            .reservations
            .remove(&res.0)
            .ok_or(SpineError::NoSuchReservation)?;
        let kb = g
            .budgets
            .get_mut(&r.key_id)
            .ok_or_else(|| SpineError::NoSuchKey {
                key_id: r.key_id.clone(),
            })?;
        kb.reserved -= r.estimate;
        kb.spent += actual;
        Ok(())
    }

    /// Drop a reservation that never billed (e.g. request failed pre-call).
    pub fn release(&self, res: ReservationId) -> Result<(), SpineError> {
        let mut g = self.inner.lock().unwrap();
        let r = g
            .reservations
            .remove(&res.0)
            .ok_or(SpineError::NoSuchReservation)?;
        if let Some(kb) = g.budgets.get_mut(&r.key_id) {
            kb.reserved -= r.estimate;
        }
        Ok(())
    }

    pub fn spent(&self, key_id: &str) -> Usd {
        let g = self.inner.lock().unwrap();
        g.budgets
            .get(key_id)
            .map(|kb| kb.spent)
            .unwrap_or(Usd::ZERO)
    }

    pub fn reserved(&self, key_id: &str) -> Usd {
        let g = self.inner.lock().unwrap();
        g.budgets
            .get(key_id)
            .map(|kb| kb.reserved)
            .unwrap_or(Usd::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_key_cannot_reserve() {
        let l = BudgetLedger::new();
        assert!(matches!(
            l.reserve("ghost", Usd::from_micros(1)),
            Err(SpineError::NoSuchKey { .. })
        ));
    }

    #[test]
    fn unlimited_budget_always_reserves() {
        let l = BudgetLedger::new();
        l.set_budget("k", None, Usd::ZERO);
        for _ in 0..1000 {
            assert!(l.reserve("k", Usd::from_dollars_f64(1000.0)).is_ok());
        }
    }

    #[test]
    fn reserve_commit_trues_up_actual() {
        let l = BudgetLedger::new();
        l.set_budget("k", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);
        let r = l.reserve("k", Usd::from_dollars_f64(0.50)).unwrap();
        assert_eq!(l.reserved("k"), Usd::from_dollars_f64(0.50));
        // actual came in lower than the estimate
        l.commit(r, Usd::from_dollars_f64(0.30)).unwrap();
        assert_eq!(l.reserved("k"), Usd::ZERO);
        assert_eq!(l.spent("k"), Usd::from_dollars_f64(0.30));
    }

    #[test]
    fn reserve_is_fail_closed_at_budget() {
        let l = BudgetLedger::new();
        l.set_budget("k", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);
        // reserve $0.60 then $0.60 → second must fail (0.60 + 0.60 > 1.00)
        let _r1 = l.reserve("k", Usd::from_dollars_f64(0.60)).unwrap();
        assert!(matches!(
            l.reserve("k", Usd::from_dollars_f64(0.60)),
            Err(SpineError::BudgetExceeded { .. })
        ));
    }

    #[test]
    fn release_frees_the_reservation() {
        let l = BudgetLedger::new();
        l.set_budget("k", Some(Usd::from_dollars_f64(1.0)), Usd::ZERO);
        let r = l.reserve("k", Usd::from_dollars_f64(0.90)).unwrap();
        l.release(r).unwrap();
        assert_eq!(l.reserved("k"), Usd::ZERO);
        // full budget available again
        assert!(l.reserve("k", Usd::from_dollars_f64(0.90)).is_ok());
    }

    #[test]
    fn double_commit_is_rejected() {
        let l = BudgetLedger::new();
        l.set_budget("k", None, Usd::ZERO);
        let r = l.reserve("k", Usd::from_micros(1)).unwrap();
        l.commit(r, Usd::from_micros(1)).unwrap();
        assert!(matches!(
            l.commit(r, Usd::from_micros(1)),
            Err(SpineError::NoSuchReservation)
        ));
    }
}
