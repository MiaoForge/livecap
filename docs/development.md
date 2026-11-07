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

See [repository instructions](../AGENTS.md) for contribution conventions and [releases](releases.md) for distribution.
