"""Business logic: a bank ledger with typed errors and a double-entry journal.

Applies N operations (transfers, deposits, withdrawals) to 1,000 named
accounts. Each operation can fail: an unknown account, a frozen account,
insufficient funds, or an amount over the limit. Python raises exceptions, Rust
and Aura return typed errors. Every successful posting goes to a journal, and
interest is credited to every account every 10,000 operations. Reports the
outcome counts, the money supply, and a journal checksum.
"""
import sys

ACCOUNTS = 1000
LIMIT = 5000
MOD = 1000000007


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


class LedgerError(Exception):
    pass


class UnknownAccount(LedgerError):
    def __init__(self, name):
        super().__init__(name)
        self.name = name


class AccountFrozen(LedgerError):
    def __init__(self, name):
        super().__init__(name)
        self.name = name


class InsufficientFunds(LedgerError):
    def __init__(self, needed):
        super().__init__(needed)
        self.needed = needed


class LimitExceeded(LedgerError):
    def __init__(self, amount):
        super().__init__(amount)
        self.amount = amount


class Bank:
    def __init__(self):
        self.index = {}
        self.balances = []
        self.frozen = []
        self.journal_accounts = []
        self.journal_deltas = []
        for i in range(ACCOUNTS):
            self.index["ACC-" + str(i)] = i
            self.balances.append(1000 + i % 500)
            self.frozen.append(i % 97 == 0)

    def account(self, name):
        slot = self.index.get(name)
        if slot is None:
            raise UnknownAccount(name)
        if self.frozen[slot]:
            raise AccountFrozen(name)
        return slot

    def post(self, slot, delta):
        self.balances[slot] += delta
        self.journal_accounts.append(slot)
        self.journal_deltas.append(delta)

    def transfer(self, source, target, amount):
        if amount > LIMIT:
            raise LimitExceeded(amount)
        s = self.account(source)
        t = self.account(target)
        if self.balances[s] < amount:
            raise InsufficientFunds(amount - self.balances[s])
        self.post(s, -amount)
        self.post(t, amount)

    def deposit(self, target, amount):
        if amount > LIMIT:
            raise LimitExceeded(amount)
        self.post(self.account(target), amount)

    def withdraw(self, source, amount):
        if amount > LIMIT:
            raise LimitExceeded(amount)
        s = self.account(source)
        if self.balances[s] < amount:
            raise InsufficientFunds(amount - self.balances[s])
        self.post(s, -amount)

    def apply_interest(self):
        credited = 0
        for slot in range(ACCOUNTS):
            if not self.frozen[slot] and self.balances[slot] > 0:
                interest = self.balances[slot] * 5 // 10000
                if interest > 0:
                    self.post(slot, interest)
                    credited += interest
        return credited


def main():
    n = int(sys.argv[1])
    rng = Rng(59)
    bank = Bank()
    opening = sum(bank.balances)
    ok = unknown = frozen = insufficient = over_limit = 0
    shortfall = 0
    deposited = withdrawn = interest = 0
    for step in range(n):
        r = rng.next()
        kind = r % 10
        source = "ACC-" + str((r // 10) % 1100)
        target = "ACC-" + str((r // 11000) % ACCOUNTS)
        amount = 1 + (r // 7) % 6000
        try:
            if kind < 6:
                bank.transfer(source, target, amount)
            elif kind < 8:
                bank.deposit(source, amount)
                deposited += amount
            else:
                bank.withdraw(source, amount)
                withdrawn += amount
            ok += 1
        except UnknownAccount:
            unknown += 1
        except AccountFrozen:
            frozen += 1
        except InsufficientFunds as error:
            insufficient += 1
            shortfall += error.needed
        except LimitExceeded:
            over_limit += 1
        if step % 10000 == 9999:
            interest += bank.apply_interest()
    supply = sum(bank.balances)
    balanced = supply == opening + deposited - withdrawn + interest
    checksum = 0
    for i in range(len(bank.journal_deltas)):
        checksum = (checksum * 31 + bank.journal_accounts[i] * 7 + bank.journal_deltas[i]) % MOD
    print(
        f"bank_ledger ops={n} ok={ok} unknown={unknown} frozen={frozen} insufficient={insufficient} "
        f"over_limit={over_limit} shortfall={shortfall} supply={supply} balanced={str(balanced).lower()} "
        f"entries={len(bank.journal_deltas)} checksum={checksum}"
    )


if __name__ == "__main__":
    main()
