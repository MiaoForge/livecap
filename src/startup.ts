export type StartupResource =
  | "settings"
  | "openaiKey"
  | "sonioxKey"
  | "libraries"
  | "records"
  | "usage"
  | "knowledgeUsage";

export async function loadStartupResources<
  T extends Record<string, () => Promise<unknown>>,
>(
  loaders: T,
): Promise<{
  [K in keyof T]: PromiseSettledResult<Awaited<ReturnType<T[K]>>>;
}> {
  const entries = Object.entries(loaders);
  const results = await Promise.allSettled(
    entries.map(([, load]) => Promise.resolve().then(load)),
  );
  return Object.fromEntries(
    entries.map(([key], index) => [key, results[index]]),
  ) as {
    [K in keyof T]: PromiseSettledResult<Awaited<ReturnType<T[K]>>>;
  };
}
