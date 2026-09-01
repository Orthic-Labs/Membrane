export type Order = { sku: string; status: string };

export class OrderStore {
  private readonly rows = new Map<string, Order>();

  find(sku: string) {
    return this.rows.get(sku);
  }

  save(order: Order) {
    this.rows.set(order.sku, order);
    return order;
  }
}
