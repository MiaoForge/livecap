# Development

Use the Windows prerequisites in the [project README](../README.md#quick-start). Install the Rust quality tools and JavaScript dependencies from the repository root.

```powershell
rustup component add rustfmt clippy
npm ci
npm run tauri dev
```

Use `npm run dev` for a browser layout preview. Desktop features such as system audio capture, Windows credentials, and the local database require the Tauri app.

| Command                                                  | Purpose                                                   |
| -------------------------------------------------------- | --------------------------------------------------------- |
| `npm run check`                                          | Run the checks used by CI                                 |
| `npm run format`                                         | Format maintained frontend, documentation, and Rust files |
| `npm run format:check`                                   | Verify Prettier and rustfmt formatting                    |
| `npm run lint`                                           | Run ESLint and Clippy                                     |
| `npm test`                                               | Run frontend tests                                        |
| `npm run test:rust`                                      | Run locked Rust tests                                     |
| `npm run tauri -- build --debug --no-bundle -- --locked` | Build the desktop executable                              |

CI checks pull requests and pushes to `main` on Windows with npm and Rust caches. Release builds run the same checks before signing the installer. The test suite covers application behavior with fixtures and local test services. Live provider quality and native UI behavior need separate verification.

## Dependency maintenance

The npm overrides in `package.json` keep Excalidraw 0.18.1 compatible with React 19 through Radix Tabs 1.2.1 and replace vulnerable transitive versions of nanoid, KaTeX, lodash-es, and minimatch. Keep nanoid 3 for Excalidraw and nanoid 5 for the Mermaid converter; their supported module formats differ. Revisit these overrides when upgrading the parent packages. Validate changes with a clean `npm ci`, `npm audit --registry=https://registry.npmjs.org`, `npm run check`, and a browser check of diagram rendering and export.

`MeetingDiagram` imports Excalidraw on demand. Excalidraw and Mermaid ship prebundled modules, including an approximately 1.82 MB minified module that Vite cannot split further. The build therefore uses a 2,000 kB chunk warning budget while retaining warnings above that budget. This budget does not reduce bundle size; review it when upgrading the diagram libraries and keep diagram loading separate from application startup.

See [repository instructions](../AGENTS.md) for contribution conventions and [releases](releases.md) for distribution.
