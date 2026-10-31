"use client";

import type { AuthProvider } from "@lattice/shared-types";

import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { GATEWAY_URL } from "@/lib/gateway";
import { useAuthProviders } from "@/lib/queries";

/** The providers the design shows, in its order. Others the gateway reports are appended. */
const DESIGNED: AuthProvider[] = [
  { id: "google", label: "Continue with Google" },
  { id: "microsoft", label: "Continue with Microsoft" },
  { id: "sso", label: "Single sign-on (SSO)" },
];

export function SsoButtons({ remember }: { remember: boolean }) {
  const { data } = useAuthProviders();
  const configured = new Map((data?.providers ?? []).map((p) => [p.id, p]));
  const extra = (data?.providers ?? []).filter((p) => !DESIGNED.some((d) => d.id === p.id));

  return (
    <div className="flex flex-col gap-2.5">
      {[...DESIGNED, ...extra].map((designed) => {
        const provider = configured.get(designed.id);
        if (provider) {
          return (
            <Button
              key={provider.id}
              asChild
              variant="outline"
              size="lg"
              className="font-medium text-foreground hover:text-foreground"
            >
              <a href={`${GATEWAY_URL}/api/auth/oidc/${provider.id}/start?remember=${remember}`}>
                {provider.label}
              </a>
            </Button>
          );
        }
        return (
          <Tooltip key={designed.id}>
            <TooltipTrigger asChild>
              {/* Disabled buttons swallow pointer events; the wrapper keeps the tooltip reachable. */}
              <span tabIndex={0} className="flex rounded-[10px]">
                <Button variant="outline" size="lg" disabled className="w-full font-medium">
                  {designed.label}
                </Button>
              </span>
            </TooltipTrigger>
            <TooltipContent>Not configured on this deployment</TooltipContent>
          </Tooltip>
        );
      })}
    </div>
  );
}
