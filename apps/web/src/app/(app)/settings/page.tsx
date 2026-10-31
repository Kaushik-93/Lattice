import type { Metadata } from "next";

import { SettingsView } from "@/components/settings/settings-view";

export const metadata: Metadata = { title: "Settings" };

export default function SettingsPage() {
  return (
    <>
      <h1 className="text-[28px] leading-[1.2] font-semibold tracking-[-0.02em]">Settings</h1>
      <SettingsView />
    </>
  );
}
