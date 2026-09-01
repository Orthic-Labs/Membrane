import { OrderStore } from "./store.js";

export class OrderService {
  constructor(private readonly store: OrderStore) {}

  placeOrder(sku: string) {
    const existing = this.store.find(sku);
    return existing ?? this.store.save({ sku, status: "created" });
  }
}
