import type { ReactNode } from "react";

import { AuthAside } from "@/components/auth/auth-aside";

/** Design 01: brand story on the left, the form on the right; stacks on narrow screens. */
export default function AuthLayout({ children }: { children: ReactNode }) {
  return (
    <div className="flex min-h-screen flex-wrap bg-background">
      <AuthAside />
      <main className="flex min-w-0 flex-[1_1_520px] items-center justify-center px-8 py-14">
        <div className="flex w-full max-w-[400px] flex-col gap-7">{children}</div>
      </main>
    </div>
  );
}
