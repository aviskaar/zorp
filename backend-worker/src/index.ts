import { Container, getContainer } from "@cloudflare/containers";

interface Env {
  ZORP_BACKEND: DurableObjectNamespace<ZorpBackend>;
  ZORP_WEB_TOKEN: string;
  ZORP_ALLOW_ORIGIN: string;
  ZORP_BASE_URL: string;
  ZORP_MODEL: string;
  ZORP_API_KEY: string;
}

export class ZorpBackend extends Container<Env> {
  defaultPort = 7777;
  sleepAfter = "30m";

  // `envVars` cannot be a class-field literal here: it needs `this.env`,
  // which the base Container constructor sets up.
  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.envVars = {
      ZORP_WEB_TOKEN: env.ZORP_WEB_TOKEN,
      ZORP_ALLOW_ORIGIN: env.ZORP_ALLOW_ORIGIN,
      ZORP_BASE_URL: env.ZORP_BASE_URL,
      ZORP_MODEL: env.ZORP_MODEL,
      ZORP_API_KEY: env.ZORP_API_KEY,
    };
  }
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    // One instance for this test deploy: getContainer's default name
    // ("cf-singleton-container") pins every request to the same instance
    // rather than spreading across many.
    return getContainer(env.ZORP_BACKEND).fetch(request);
  },
};
