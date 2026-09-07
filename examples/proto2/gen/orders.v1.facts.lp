grade(orders(r0, 0), a).
id(orders(r0, 0), "PO-1001").
id(orders(r0, 1), "PO-1002").
ledger(r0).
order(orders(r0, 0)).
order(orders(r0, 1)).
quantity(orders(r0, 0), 5).
tags(orders(r0, 0), 0, "rush").
tags(orders(r0, 0), 1, "fragile").
tags(orders(r0, 1), 0, "bulk").
