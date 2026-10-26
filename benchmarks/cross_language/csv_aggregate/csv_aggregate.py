"""CSV analytics: parse a sales export and aggregate it by region, product, and month.

Generates N rows of CSV text, then parses every line, computes discounted
revenue, groups it three ways, and ranks the five best-selling products.
"""
import sys

REGIONS = ["north", "south", "east", "west", "central", "coastal"]


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def two_digits(value):
    return "0" + str(value) if value < 10 else str(value)


def generate(rng, n):
    lines = ["date,region,product,quantity,unit_price_cents,discount_pct"]
    for _ in range(n):
        r = rng.next()
        s = rng.next()
        month = two_digits(1 + r % 12)
        day = two_digits(1 + (r // 12) % 28)
        region = REGIONS[(r // 336) % 6]
        product = f"SKU-{s % 500}"
        quantity = 1 + (s // 500) % 50
        price = 100 + (s // 25000) % 99900
        discount = (r // 2016) % 31
        lines.append(f"2026-{month}-{day},{region},{product},{quantity},{price},{discount}")
    return "\n".join(lines) + "\n"


def main():
    n = int(sys.argv[1])
    text = generate(Rng(17), n)
    revenue_by_region = {}
    quantity_by_product = {}
    orders_by_month = {}
    rows = 0
    total_revenue = 0
    for line in text.splitlines()[1:]:
        fields = line.split(",")
        month = fields[0].split("-")[1]
        quantity = int(fields[3])
        price = int(fields[4])
        discount = int(fields[5])
        revenue = quantity * price * (100 - discount) // 100
        rows += 1
        total_revenue += revenue
        revenue_by_region[fields[1]] = revenue_by_region.get(fields[1], 0) + revenue
        quantity_by_product[fields[2]] = quantity_by_product.get(fields[2], 0) + quantity
        orders_by_month[month] = orders_by_month.get(month, 0) + 1
    ranked = list(quantity_by_product.items())
    ranked.sort(key=lambda entry: -entry[1])
    top = " ".join(f"{product}:{quantity}" for product, quantity in ranked[:5])
    regions = " ".join(f"{region}:{revenue_by_region.get(region, 0)}" for region in REGIONS)
    busiest = max(orders_by_month.values())
    print(
        f"csv_aggregate rows={rows} bytes={len(text)} revenue={total_revenue} "
        f"products={len(quantity_by_product)} busiest_month={busiest} regions={regions} top={top}"
    )


if __name__ == "__main__":
    main()
