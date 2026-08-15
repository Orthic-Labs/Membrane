export type LifecycleProductId = "cortex" | "membrane";

/** Closed lifecycle states (orthic.lifecycle.v1 §4.2). */
export type LifecycleState =
  | "starting"
  | "ready"
  | "degraded"
  | "draining"
  | "stopped"
  | "incompatible"
  | "failed";

/** States a child may self-report in a register frame. */
export type LifecycleRegisterState =
  | "starting"
  | "ready"
  | "degraded"
  | "incompatible"
  | "failed";

/** Closed lifecycle commands: drain, stop, update handoff, ownership-loss acknowledgement. */
export type LifecycleCommand = "drain" | "stop" | "update_handoff" | "ownership_loss";

export interface LoopbackEndpoint {
  host?: "127.0.0.1" | "localhost" | "::1";
  port: number;
}

/** Parent -> child binding frame. The secret exists only on the inherited pipe. */
export interface HelloFrame {
  kind: "hello";
  lifecycleVersion: 1;
  installationId: string;
  productId: LifecycleProductId;
  instanceId: string;
  fence: number;
  artifactDigest: `sha256:${string}`;
  declaredDataRoot: string;
  secret: string;
}

/** Child -> parent readiness frame. Ready requires both endpoint and capability. */
export interface RegisterFrame {
  kind: "register";
  state: LifecycleRegisterState;
  fence: number;
  endpoint?: LoopbackEndpoint;
  capability?: string;
}

/** Parent -> child lifecycle command, accepted only with the matching exact fence. */
export interface CommandFrame {
  kind: "command";
  command: LifecycleCommand;
  fence: number;
}

/** Child -> parent exact-fence acknowledgement of a lifecycle command. */
export interface AckFrame {
  kind: "ack";
  command: LifecycleCommand;
  fence: number;
}

export type LifecycleFrame = HelloFrame | RegisterFrame | CommandFrame | AckFrame;
