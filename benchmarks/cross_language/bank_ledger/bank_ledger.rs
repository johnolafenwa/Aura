//! Business logic: a bank ledger with typed errors and a double-entry journal.
//!
//! Applies N operations (transfers, deposits, withdrawals) to 1,000 named
//! accounts. Each operation can fail: an unknown account, a frozen account,
//! insufficient funds, or an amount over the limit. Python raises exceptions, Rust
//! and Aura return typed errors. Every successful posting goes to a journal, and
//! interest is credited to every account every 10,000 operations. Reports the
//! outcome counts, the money supply, and a journal checksum.
use std::collections::HashMap;

const ACCOUNTS: usize = 1000;
const LIMIT: i64 = 5000;
const MOD: i64 = 1000000007;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

#[allow(dead_code)]
enum LedgerError {
    UnknownAccount(String),
    AccountFrozen(String),
    InsufficientFunds(i64),
    LimitExceeded(i64),
}

struct Bank {
    index: HashMap<String, usize>,
    balances: Vec<i64>,
    frozen: Vec<bool>,
    journal_accounts: Vec<usize>,
    journal_deltas: Vec<i64>,
}

impl Bank {
    fn new() -> Self {
        let mut bank = Bank {
            index: HashMap::new(),
            balances: Vec::new(),
            frozen: Vec::new(),
            journal_accounts: Vec::new(),
            journal_deltas: Vec::new(),
        };
        for i in 0..ACCOUNTS {
            bank.index.insert(format!("ACC-{i}"), i);
            bank.balances.push(1000 + (i % 500) as i64);
            bank.frozen.push(i % 97 == 0);
        }
        bank
    }

    fn account(&self, name: &str) -> Result<usize, LedgerError> {
        let slot = *self
            .index
            .get(name)
            .ok_or_else(|| LedgerError::UnknownAccount(name.to_string()))?;
        if self.frozen[slot] {
            return Err(LedgerError::AccountFrozen(name.to_string()));
        }
        Ok(slot)
    }

    fn post(&mut self, slot: usize, delta: i64) {
        self.balances[slot] += delta;
        self.journal_accounts.push(slot);
        self.journal_deltas.push(delta);
    }

    fn transfer(&mut self, source: &str, target: &str, amount: i64) -> Result<(), LedgerError> {
        if amount > LIMIT {
            return Err(LedgerError::LimitExceeded(amount));
        }
        let s = self.account(source)?;
        let t = self.account(target)?;
        if self.balances[s] < amount {
            return Err(LedgerError::InsufficientFunds(amount - self.balances[s]));
        }
        self.post(s, -amount);
        self.post(t, amount);
        Ok(())
    }

    fn deposit(&mut self, target: &str, amount: i64) -> Result<(), LedgerError> {
        if amount > LIMIT {
            return Err(LedgerError::LimitExceeded(amount));
        }
        let t = self.account(target)?;
        self.post(t, amount);
        Ok(())
    }

    fn withdraw(&mut self, source: &str, amount: i64) -> Result<(), LedgerError> {
        if amount > LIMIT {
            return Err(LedgerError::LimitExceeded(amount));
        }
        let s = self.account(source)?;
        if self.balances[s] < amount {
            return Err(LedgerError::InsufficientFunds(amount - self.balances[s]));
        }
        self.post(s, -amount);
        Ok(())
    }

    fn apply_interest(&mut self) -> i64 {
        let mut credited = 0;
        for slot in 0..ACCOUNTS {
            if !self.frozen[slot] && self.balances[slot] > 0 {
                let interest = self.balances[slot] * 5 / 10000;
                if interest > 0 {
                    self.post(slot, interest);
                    credited += interest;
                }
            }
        }
        credited
    }
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut rng = Rng { state: 59 };
    let mut bank = Bank::new();
    let opening: i64 = bank.balances.iter().sum();
    let (mut ok, mut unknown, mut frozen, mut insufficient, mut over_limit) = (0, 0, 0, 0, 0);
    let (mut shortfall, mut deposited, mut withdrawn, mut interest) = (0i64, 0i64, 0i64, 0i64);
    for step in 0..n {
        let r = rng.next();
        let kind = r % 10;
        let source = format!("ACC-{}", (r / 10) % 1100);
        let target = format!("ACC-{}", (r / 11000) % ACCOUNTS as i64);
        let amount = 1 + (r / 7) % 6000;
        let outcome = if kind < 6 {
            bank.transfer(&source, &target, amount)
        } else if kind < 8 {
            bank.deposit(&source, amount).map(|_| deposited += amount)
        } else {
            bank.withdraw(&source, amount).map(|_| withdrawn += amount)
        };
        match outcome {
            Ok(()) => ok += 1,
            Err(LedgerError::UnknownAccount(_)) => unknown += 1,
            Err(LedgerError::AccountFrozen(_)) => frozen += 1,
            Err(LedgerError::InsufficientFunds(needed)) => {
                insufficient += 1;
                shortfall += needed;
            }
            Err(LedgerError::LimitExceeded(_)) => over_limit += 1,
        }
        if step % 10000 == 9999 {
            interest += bank.apply_interest();
        }
    }
    let supply: i64 = bank.balances.iter().sum();
    let balanced = supply == opening + deposited - withdrawn + interest;
    let mut checksum = 0i64;
    for i in 0..bank.journal_deltas.len() {
        checksum =
            (checksum * 31 + bank.journal_accounts[i] as i64 * 7 + bank.journal_deltas[i]) % MOD;
    }
    println!(
        "bank_ledger ops={n} ok={ok} unknown={unknown} frozen={frozen} insufficient={insufficient} \
         over_limit={over_limit} shortfall={shortfall} supply={supply} balanced={balanced} entries={} checksum={checksum}",
        bank.journal_deltas.len()
    );
}
