import type { Metadata } from "next";
import Link from "next/link";
import "./globals.css";

export const metadata: Metadata = {
  title: "AI Usage Controller — never get cut off, never overpay for AI",
  description: "A Mac and Windows widget that shows how much Claude Code, Codex and Gemini runway you have left, warns you before you get cut off, and tells you when you are wasting money.",
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className="min-h-screen">
        <header className="mx-auto flex max-w-5xl items-center justify-between px-4 py-5 sm:px-6">
          <Link href="/" className="flex items-center gap-2 font-semibold">
            <span className="inline-block h-2.5 w-8 overflow-hidden rounded-full bg-line">
              <span className="block h-full w-5 rounded-full bg-ok" />
            </span>
            AI Usage Controller
          </Link>
          <nav className="flex gap-5 text-sm text-mute">
            <Link href="/calculator" className="hover:text-white">Waste calculator</Link>
            <Link href="/#pricing" className="hover:text-white">Pricing</Link>
            <Link href="/dashboard" className="hover:text-white">Team dashboard</Link>
          </nav>
        </header>
        {children}
        <footer className="mx-auto max-w-5xl px-4 py-10 text-xs text-mute sm:px-6">
          We never read prompts or code. Only token counts, timestamps and model names leave your machine, and only if you turn sync on.
        </footer>
      </body>
    </html>
  );
}
