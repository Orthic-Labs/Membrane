# Architecture

`POST /orders` enters at `src/routes.ts`, then calls `OrderService.placeOrder`.

The order store persists every order in Redis.
