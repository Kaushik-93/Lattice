import type { Metadata, Viewport } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import type { ReactNode } from "react";

import { PerfOverlay } from "@/components/perf/perf-overlay";

import { Providers } from "./providers";
import "./globals.css";

// Self-hosted at build time by next/font: no runtime request to Google, no layout shift.
const geistSans = Geist({ variable: "--font-geist-sans", subsets: ["latin"] });
const geistMono = Geist_Mono({ variable: "--font-geist-mono", subsets: ["latin"] });

export const metadata: Metadata = {
  title: { default: "Lattice", template: "%s · Lattice" },
  description: "Ask your data. Keep the logic.",
};

export const viewport: Viewport = {
  themeColor: "#f6f7f9",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className={`${geistSans.variable} ${geistMono.variable}`}>
      <body>
        <Providers>
          {children}
          <PerfOverlay />
        </Providers>
      </body>
    </html>
  );
}
