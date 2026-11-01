import { describe, expect, it } from "vitest";

import { fiscalQuarterLabel, fiscalYearLabel, presetRange } from "./fiscal";

const at = (iso: string) => Date.parse(`${iso}T00:00:00Z`);

describe("the fiscal calendar", () => {
  it("names fiscal periods after the year they end in", () => {
    // February year: FY2027 runs Feb 2026–Jan 2027; Q3 is Aug–Oct.
    expect(fiscalQuarterLabel(at("2026-08-01"), 2)).toBe("Q3 FY2027");
    expect(fiscalQuarterLabel(at("2026-02-01"), 2)).toBe("Q1 FY2027");
    expect(fiscalQuarterLabel(at("2025-11-01"), 2)).toBe("Q4 FY2026");
    expect(fiscalYearLabel(at("2026-02-01"), 2)).toBe("FY2027");
    // A January year is the calendar year.
    expect(fiscalQuarterLabel(at("2026-07-01"), 1)).toBe("Q3 FY2026");
    expect(fiscalYearLabel(at("2026-01-01"), 1)).toBe("FY2026");
  });

  it("works out ranges to date on the fiscal calendar", () => {
    const today = new Date(2026, 9, 15); // 15 Oct 2026, local time
    expect(presetRange("fiscal_year_to_date", 2, today)).toEqual(["2026-02-01", "2026-10-15"]);
    expect(presetRange("fiscal_quarter_to_date", 2, today)).toEqual(["2026-08-01", "2026-10-15"]);
    expect(presetRange("month_to_date", 2, today)).toEqual(["2026-10-01", "2026-10-15"]);
    // January, early in a February year: the fiscal year began last February.
    const january = new Date(2027, 0, 10);
    expect(presetRange("fiscal_year_to_date", 2, january)).toEqual(["2026-02-01", "2027-01-10"]);
    expect(presetRange("fiscal_quarter_to_date", 2, january)).toEqual(["2026-11-01", "2027-01-10"]);
  });
});
