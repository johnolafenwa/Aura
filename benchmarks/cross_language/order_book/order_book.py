"""Trading: a limit order book with price-time priority matching.

Feeds N events into a book with 1,000 price ticks: limit orders (60%), cancels
of earlier orders (25%), and market orders (15%) around a drifting mid price.
Each price level keeps a FIFO of order ids with a head index; cancelled orders
are skipped lazily when they reach the head. Reports trades, volume, notional
value, cancels, resting orders, the final best bid and ask, and a checksum.
"""
import sys

TICKS = 1000
BUY = 0
SELL = 1
MOD = 1000000007


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


class OrderBook:
    def __init__(self):
        self.order_qty = []
        self.order_price = []
        self.order_side = []
        self.bids = [[] for _ in range(TICKS)]
        self.asks = [[] for _ in range(TICKS)]
        self.bid_heads = [0] * TICKS
        self.ask_heads = [0] * TICKS
        self.bid_qty = [0] * TICKS
        self.ask_qty = [0] * TICKS
        self.best_bid = -1
        self.best_ask = TICKS
        self.trades = 0
        self.volume = 0
        self.notional = 0
        self.cancels = 0
        self.checksum = 0

    def fix_best_bid(self):
        while self.best_bid >= 0 and self.bid_qty[self.best_bid] == 0:
            self.best_bid -= 1

    def fix_best_ask(self):
        while self.best_ask < TICKS and self.ask_qty[self.best_ask] == 0:
            self.best_ask += 1

    def fill(self, resting, remaining, level):
        amount = min(remaining, self.order_qty[resting])
        self.order_qty[resting] -= amount
        self.trades += 1
        self.volume += amount
        self.notional += amount * level
        self.checksum = (self.checksum * 31 + resting * 7 + amount) % MOD
        return amount

    def submit(self, side, price, qty, rest):
        order_id = len(self.order_qty)
        self.order_qty.append(0)
        self.order_price.append(price)
        self.order_side.append(side)
        remaining = qty
        if side == BUY:
            while remaining > 0:
                self.fix_best_ask()
                if self.best_ask >= TICKS or self.best_ask > price:
                    break
                level = self.best_ask
                queue = self.asks[level]
                head = self.ask_heads[level]
                while self.order_qty[queue[head]] == 0:
                    head += 1
                resting = queue[head]
                amount = self.fill(resting, remaining, level)
                remaining -= amount
                self.ask_qty[level] -= amount
                if self.order_qty[resting] == 0:
                    head += 1
                self.ask_heads[level] = head
            if remaining > 0 and rest:
                self.order_qty[order_id] = remaining
                self.bids[price].append(order_id)
                self.bid_qty[price] += remaining
                if price > self.best_bid:
                    self.best_bid = price
        else:
            while remaining > 0:
                self.fix_best_bid()
                if self.best_bid < 0 or self.best_bid < price:
                    break
                level = self.best_bid
                queue = self.bids[level]
                head = self.bid_heads[level]
                while self.order_qty[queue[head]] == 0:
                    head += 1
                resting = queue[head]
                amount = self.fill(resting, remaining, level)
                remaining -= amount
                self.bid_qty[level] -= amount
                if self.order_qty[resting] == 0:
                    head += 1
                self.bid_heads[level] = head
            if remaining > 0 and rest:
                self.order_qty[order_id] = remaining
                self.asks[price].append(order_id)
                self.ask_qty[price] += remaining
                if price < self.best_ask:
                    self.best_ask = price

    def cancel(self, order_id):
        qty = self.order_qty[order_id]
        if qty == 0:
            return
        price = self.order_price[order_id]
        if self.order_side[order_id] == BUY:
            self.bid_qty[price] -= qty
        else:
            self.ask_qty[price] -= qty
        self.order_qty[order_id] = 0
        self.cancels += 1


def main():
    n = int(sys.argv[1])
    rng = Rng(47)
    book = OrderBook()
    mid = 500
    for step in range(n):
        r = rng.next()
        if step % 100 == 0:
            mid = min(900, max(100, mid + (r // 100) % 11 - 5))
        kind = r % 100
        if kind < 60:
            side = (r // 100) % 2
            offset = (r // 200) % 20
            qty = 1 + (r // 4000) % 50
            if side == BUY:
                book.submit(BUY, mid - offset + 5, qty, True)
            else:
                book.submit(SELL, mid + offset - 5, qty, True)
        elif kind < 85:
            if len(book.order_qty) > 0:
                book.cancel((r // 100) % len(book.order_qty))
        else:
            side = (r // 100) % 2
            qty = 1 + (r // 200) % 100
            if side == BUY:
                book.submit(BUY, TICKS - 1, qty, False)
            else:
                book.submit(SELL, 0, qty, False)
    book.fix_best_bid()
    book.fix_best_ask()
    resting = 0
    for qty in book.order_qty:
        if qty > 0:
            resting += 1
    print(
        f"order_book events={n} orders={len(book.order_qty)} trades={book.trades} volume={book.volume} "
        f"notional={book.notional} cancels={book.cancels} resting={resting} best_bid={book.best_bid} "
        f"best_ask={book.best_ask} checksum={book.checksum}"
    )


if __name__ == "__main__":
    main()
