import { OrderService } from "../src/service.js";
import { OrderStore } from "../src/store.js";

test("placeOrder stores a new order", () => {
  const service = new OrderService(new OrderStore());
  expect(service.placeOrder("sku-1").status).toBe("created");
});
