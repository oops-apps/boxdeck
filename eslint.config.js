import js from "@eslint/js";
import prettier from "eslint-config-prettier";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist", "node_modules", "target", "src-tauri", "src/ipc/generated"] },
  {
    files: ["**/*.{ts,tsx}"],
    extends: [js.configs.recommended, ...tseslint.configs.strict],
    languageOptions: { globals: globals.browser },
    plugins: { "react-hooks": reactHooks },
    rules: reactHooks.configs.recommended.rules,
  },
  prettier,
);
