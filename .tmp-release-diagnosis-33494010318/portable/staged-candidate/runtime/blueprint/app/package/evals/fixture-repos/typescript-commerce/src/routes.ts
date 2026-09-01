import type { OrderService } from "./service.js";

export function registerOrderRoute(router: any, service: OrderService) {
  router.post("/orders", (req: any, res: any) => {
    const result = service.placeOrder(req.body.sku);
    return res.json(result);
  });
}
