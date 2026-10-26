//! Trading: a limit order book with price-time priority matching.
//!
//! Feeds N events into a book with 1,000 price ticks: limit orders (60%), cancels
//! of earlier orders (25%), and market orders (15%) around a drifting mid price.
//! Each price level keeps a FIFO of order ids with a head index; cancelled orders
//! are skipped lazily when they reach the head. Reports trades, volume, notional
//! value, cancels, resting orders, the final best bid and ask, and a checksum.

const TICKS: i64 = 1000;
const BUY: i64 = 0;
const SELL: i64 = 1;
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

struct OrderBook {
    order_qty: Vec<i64>,
    order_price: Vec<i64>,
    order_side: Vec<i64>,
    bids: Vec<Vec<usize>>,
    asks: Vec<Vec<usize>>,
    bid_heads: Vec<usize>,
    ask_heads: Vec<usize>,
    bid_qty: Vec<i64>,
    ask_qty: Vec<i64>,
    best_bid: i64,
    best_ask: i64,
    trades: i64,
    volume: i64,
    notional: i64,
    cancels: i64,
    checksum: i64,
}

impl OrderBook {
    fn new() -> Self {
        let ticks = TICKS as usize;
        OrderBook {
            order_qty: Vec::new(),
            order_price: Vec::new(),
            order_side: Vec::new(),
            bids: vec![Vec::new(); ticks],
            asks: vec![Vec::new(); ticks],
            bid_heads: vec![0; ticks],
            ask_heads: vec![0; ticks],
            bid_qty: vec![0; ticks],
            ask_qty: vec![0; ticks],
            best_bid: -1,
            best_ask: TICKS,
            trades: 0,
            volume: 0,
            notional: 0,
            cancels: 0,
            checksum: 0,
        }
    }

    fn fix_best_bid(&mut self) {
        while self.best_bid >= 0 && self.bid_qty[self.best_bid as usize] == 0 {
            self.best_bid -= 1;
        }
    }

    fn fix_best_ask(&mut self) {
        while self.best_ask < TICKS && self.ask_qty[self.best_ask as usize] == 0 {
            self.best_ask += 1;
        }
    }

    fn fill(&mut self, resting: usize, remaining: i64, level: i64) -> i64 {
        let amount = remaining.min(self.order_qty[resting]);
        self.order_qty[resting] -= amount;
        self.trades += 1;
        self.volume += amount;
        self.notional += amount * level;
        self.checksum = (self.checksum * 31 + resting as i64 * 7 + amount) % MOD;
        amount
    }

    fn submit(&mut self, side: i64, price: i64, qty: i64, rest: bool) {
        let order_id = self.order_qty.len();
        self.order_qty.push(0);
        self.order_price.push(price);
        self.order_side.push(side);
        let mut remaining = qty;
        if side == BUY {
            while remaining > 0 {
                self.fix_best_ask();
                if self.best_ask >= TICKS || self.best_ask > price {
                    break;
                }
                let level = self.best_ask as usize;
                let mut head = self.ask_heads[level];
                while self.order_qty[self.asks[level][head]] == 0 {
                    head += 1;
                }
                let resting = self.asks[level][head];
                let amount = self.fill(resting, remaining, level as i64);
                remaining -= amount;
                self.ask_qty[level] -= amount;
                if self.order_qty[resting] == 0 {
                    head += 1;
                }
                self.ask_heads[level] = head;
            }
            if remaining > 0 && rest {
                self.order_qty[order_id] = remaining;
                self.bids[price as usize].push(order_id);
                self.bid_qty[price as usize] += remaining;
                if price > self.best_bid {
                    self.best_bid = price;
                }
            }
        } else {
            while remaining > 0 {
                self.fix_best_bid();
                if self.best_bid < 0 || self.best_bid < price {
                    break;
                }
                let level = self.best_bid as usize;
                let mut head = self.bid_heads[level];
                while self.order_qty[self.bids[level][head]] == 0 {
                    head += 1;
                }
                let resting = self.bids[level][head];
                let amount = self.fill(resting, remaining, level as i64);
                remaining -= amount;
                self.bid_qty[level] -= amount;
                if self.order_qty[resting] == 0 {
                    head += 1;
                }
                self.bid_heads[level] = head;
            }
            if remaining > 0 && rest {
                self.order_qty[order_id] = remaining;
                self.asks[price as usize].push(order_id);
                self.ask_qty[price as usize] += remaining;
                if price < self.best_ask {
                    self.best_ask = price;
                }
            }
        }
    }

    fn cancel(&mut self, order_id: usize) {
        let qty = self.order_qty[order_id];
        if qty == 0 {
            return;
        }
        let price = self.order_price[order_id] as usize;
        if self.order_side[order_id] == BUY {
            self.bid_qty[price] -= qty;
        } else {
            self.ask_qty[price] -= qty;
        }
        self.order_qty[order_id] = 0;
        self.cancels += 1;
    }
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut rng = Rng { state: 47 };
    let mut book = OrderBook::new();
    let mut mid = 500;
    for step in 0..n {
        let r = rng.next();
        if step % 100 == 0 {
            mid = (mid + (r / 100) % 11 - 5).max(100).min(900);
        }
        let kind = r % 100;
        if kind < 60 {
            let side = (r / 100) % 2;
            let offset = (r / 200) % 20;
            let qty = 1 + (r / 4000) % 50;
            if side == BUY {
                book.submit(BUY, mid - offset + 5, qty, true);
            } else {
                book.submit(SELL, mid + offset - 5, qty, true);
            }
        } else if kind < 85 {
            if !book.order_qty.is_empty() {
                let target = ((r / 100) % book.order_qty.len() as i64) as usize;
                book.cancel(target);
            }
        } else {
            let side = (r / 100) % 2;
            let qty = 1 + (r / 200) % 100;
            if side == BUY {
                book.submit(BUY, TICKS - 1, qty, false);
            } else {
                book.submit(SELL, 0, qty, false);
            }
        }
    }
    book.fix_best_bid();
    book.fix_best_ask();
    let resting = book.order_qty.iter().filter(|&&qty| qty > 0).count();
    println!(
        "order_book events={n} orders={} trades={} volume={} notional={} cancels={} resting={resting} best_bid={} \
         best_ask={} checksum={}",
        book.order_qty.len(),
        book.trades,
        book.volume,
        book.notional,
        book.cancels,
        book.best_bid,
        book.best_ask,
        book.checksum
    );
}
