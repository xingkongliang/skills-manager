import { defineConfig, type PluginOption } from "vite";
import react from "@vitejs/plugin-react";
import { inspectorServer } from "@react-dev-inspector/vite-plugin";

export default defineConfig(({ command }) => {
  const isDev = command === "serve";

  return {
    plugins: [
      /**
       * react-dev-inspector dev-server middleware: receives the clicked source
       * location from the browser and opens it in the local IDE. Dev-only.
       */
      isDev && (inspectorServer() as PluginOption),

      react({
        babel: {
          plugins: [
            /**
             * Inject `data-inspector-*` source attributes onto DOM elements so
             * the <Inspector/> can resolve the source file on click.
             * Required on React 19 (fiber `_debugSource` was removed). Dev-only.
             */
            ...(isDev ? ["@react-dev-inspector/babel-plugin"] : []),
          ],
        },
      }),
    ],
    server: {
      host: "127.0.0.1",
      port: 1420,
      strictPort: true,
      watch: {
        ignored: ["**/src-tauri/target/**"],
      },
    },
    preview: {
      host: "127.0.0.1",
      port: 1420,
      strictPort: true,
    },
  };
});
