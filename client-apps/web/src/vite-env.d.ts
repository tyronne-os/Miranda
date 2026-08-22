/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_ACE_WS_URL?: string;
  readonly VITE_ACE_HTTP_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
