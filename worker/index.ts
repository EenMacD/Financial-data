// Cloudflare Worker entry. Every request is proxied to the Rust container,
// which serves both the /api routes and the static frontend.
import { Container, getContainer } from "@cloudflare/containers";

export class Backend extends Container {
  // Must match EXPOSE / $PORT in the Dockerfile.
  defaultPort = 8080;
  // Hibernate the container after a period of inactivity (saves cost; note
  // in-memory state resets when it cold-starts again — fine for the MVP).
  sleepAfter = "30s";

  override onStart() {
    console.log("money-data container started");
  }
}

interface Env {
  BACKEND: DurableObjectNamespace<Backend>;
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    // Single shared instance for the MVP. Swap for getRandom(env.BACKEND, N)
    // to fan out across N instances once you need horizontal scale.
    return getContainer(env.BACKEND).fetch(request);
  },
};
