import type { Metadata } from "next";
import localFont from "next/font/local";
import { GeistSans } from "geist/font/sans";
import { GeistMono } from "geist/font/mono";
import "./globals.css";
import { ErrorLogging } from "@/components/error-logging";

// Fonts ship inside the app (no Google Fonts at runtime): Geist and Geist Mono from the `geist`
// package, Newsreader (variable, opsz + wght, normal and italic) from src/app/fonts.
const newsreader = localFont({
  src: [
    { path: "./fonts/newsreader-latin-opsz-normal.woff2", style: "normal", weight: "200 800" },
    { path: "./fonts/newsreader-latin-opsz-italic.woff2", style: "italic", weight: "200 800" },
  ],
  variable: "--font-newsreader",
  display: "swap",
});

export const metadata: Metadata = {
  title: "Cyber CTF",
  description: "Run Cyber CTF labs on your own machine.",
};

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    // The appearance (Dark, Black, Light) is a class on <html>, set before first paint by
    // /appearance.js (an external file: the CSP allows only same-origin scripts).
    <html lang="en" suppressHydrationWarning className={`${GeistSans.variable} ${GeistMono.variable} ${newsreader.variable} h-full antialiased`}>
      <head>
        {/* eslint-disable-next-line @next/next/no-sync-scripts -- must run before first paint to avoid a flash of the wrong mode */}
        <script src="/appearance.js" />
      </head>
      <body className="flex min-h-full flex-col bg-background text-foreground">
        <ErrorLogging />
        {children}
      </body>
    </html>
  );
}
