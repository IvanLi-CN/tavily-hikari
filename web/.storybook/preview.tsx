import type { Preview } from "@storybook/react-vite";
import React, { useEffect } from "react";

import "../src/index.css";
import { LanguageProvider, useLanguage, type Language } from "../src/i18n";

function SyncGlobals(props: { language: Language; children: React.ReactNode }): JSX.Element {
  const { language, setLanguage } = useLanguage();

  useEffect(() => {
    // Keep Storybook's toolbar state and the app language in sync.
    if (props.language !== language) setLanguage(props.language);
  }, [props.language, language, setLanguage]);

  useEffect(() => {
    // Match the app's default DaisyUI theme.
    if (typeof document !== "undefined") {
      document.documentElement.dataset.theme = "tavily";
    }
  }, []);

  return <>{props.children}</>;
}

const preview: Preview = {
  globalTypes: {
    language: {
      name: "Language",
      description: "UI language",
      defaultValue: "en",
      toolbar: {
        icon: "globe",
        items: [
          { value: "en", title: "English" },
          { value: "zh", title: "中文" },
        ],
        dynamicTitle: true,
      },
    },
  },
  decorators: [
    (Story, context) => {
      const language = (context.globals.language ?? "en") as Language;
      return (
        <LanguageProvider>
          <SyncGlobals language={language}>
            <Story />
          </SyncGlobals>
        </LanguageProvider>
      );
    },
  ],
};

export default preview;
