"use client"; // Error boundaries must be Client Components

import { ErrorScreen } from "@/components/error-screen";

/** A crash in any screen of any window (main, Settings, machine and server setup). */
export default function Error({ error, retry }: { error: Error & { digest?: string }; retry: () => void }) {
  return <ErrorScreen error={error} retry={retry} />;
}
