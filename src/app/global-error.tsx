"use client"; // Error boundaries must be Client Components

import "./globals.css";
import { ErrorScreen } from "@/components/error-screen";
import { useLocale } from "@/lib/i18n";

/** A crash in the root layout itself. It replaces the layout, so it brings its own html/body. */
export default function GlobalError({ error, retry }: { error: Error & { digest?: string }; retry: () => void }) {
  const locale = useLocale();
  return (
    <html lang={locale} className="h-full antialiased">
      <body className="min-h-full bg-background text-foreground">
        <ErrorScreen error={error} retry={retry} />
      </body>
    </html>
  );
}
