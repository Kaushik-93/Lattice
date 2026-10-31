"use client";

import { SparkleIcon } from "@/components/auth/auth-aside";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

/** Design 02's "Start a project from a question". Inert until the copilot ships (phase 4). */
export function DraftFromQuestion() {
  return (
    <form
      onSubmit={(e) => e.preventDefault()}
      className="flex flex-col gap-3 rounded-xl border border-border bg-card px-[18px] py-4"
    >
      <label htmlFor="brief" className="flex items-center gap-2 text-sm font-semibold">
        <SparkleIcon size={18} color="#4338CA" />
        Start a project from a question
      </label>
      <div className="flex flex-wrap gap-2.5">
        <input
          id="brief"
          type="text"
          disabled
          placeholder="e.g. Track weekly inventory turns by warehouse using our SQL Server and the supplier CSVs"
          className="h-11 min-w-0 flex-[1_1_360px] rounded-[10px] border border-input bg-card px-3.5 text-sm disabled:cursor-not-allowed disabled:bg-[#fafbfc]"
        />
        <Tooltip>
          <TooltipTrigger asChild>
            <span tabIndex={0} className="flex rounded-[10px]">
              <Button type="button" variant="soft" disabled className="h-11">
                Draft project
              </Button>
            </span>
          </TooltipTrigger>
          <TooltipContent>Available when the copilot ships</TooltipContent>
        </Tooltip>
      </div>
      <span className="text-xs text-subtle-foreground">
        Lattice suggests sources to connect, starter definitions and a first sheet. You review
        before anything is created.
      </span>
    </form>
  );
}
