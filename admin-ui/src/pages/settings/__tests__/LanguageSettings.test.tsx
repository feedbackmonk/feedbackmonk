import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import axios from "axios";
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { LanguageSettings } from "../LanguageSettings";
import { ToastProvider } from "../../../components/Toast";
import { renderWithClient } from "../../../test/testUtils";

// Contract C38 is MOCKED here: the endpoint is CLAUDE-C's half of the stage
// and lands after this page. The mock is the contract as written in the plan
// (`{ locale: string|null, translate_outbound: boolean }`, 400
// `{code:"invalid_locale"}`), so when the live endpoint arrives the only thing
// that changes is that these calls stop being intercepted.
vi.mock("../../../shared/localeApi", async () => {
  const actual = await vi.importActual<
    typeof import("../../../shared/localeApi")
  >("../../../shared/localeApi");
  return {
    ...actual,
    getLocaleSettings: vi.fn(),
    putLocaleSettings: vi.fn(),
  };
});

import {
  getLocaleSettings,
  putLocaleSettings,
} from "../../../shared/localeApi";
import { LOCALE_STORAGE_KEY } from "../../../i18n/useLocale";
import { i18n } from "../../../i18n";

const mockedGet = vi.mocked(getLocaleSettings);
const mockedPut = vi.mocked(putLocaleSettings);

function renderPage() {
  return renderWithClient(
    <ToastProvider>
      <LanguageSettings />
    </ToastProvider>,
    { withRouter: true, initialPath: "/admin/settings/language" },
  );
}

describe("LanguageSettings (FR-FBR-38, Contract C38)", () => {
  beforeEach(() => {
    mockedGet.mockReset();
    mockedPut.mockReset();
    window.localStorage.clear();
  });

  afterEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("shows 'Browser default' as the selected option when the tenant has no locale", async () => {
    mockedGet.mockResolvedValue({ locale: null, translate_outbound: false });
    renderPage();

    const select = (await screen.findByLabelText(
      "Default language",
    )) as HTMLSelectElement;
    expect(select.value).toBe("");
    // Every shipped locale is offered, by its own name.
    expect(
      screen.getByRole("option", { name: "Deutsch" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "فارسی" })).toBeInTheDocument();
  });

  it("preselects the tenant's stored locale", async () => {
    mockedGet.mockResolvedValue({ locale: "ja", translate_outbound: false });
    renderPage();
    const select = (await screen.findByLabelText(
      "Default language",
    )) as HTMLSelectElement;
    expect(select.value).toBe("ja");
  });

  it("PUTs the chosen locale and applies it immediately", async () => {
    mockedGet.mockResolvedValue({ locale: null, translate_outbound: false });
    mockedPut.mockResolvedValue({ locale: "de", translate_outbound: false });
    renderPage();

    const select = await screen.findByLabelText("Default language");
    fireEvent.change(select, { target: { value: "de" } });

    await waitFor(() => {
      expect(mockedPut).toHaveBeenCalledWith({ locale: "de" });
    });
    // Applied without a reload: i18next language, <html lang> and the stored
    // choice all move together.
    await waitFor(() => {
      expect(i18n.language).toBe("de");
    });
    expect(document.documentElement.lang).toBe("de");
    expect(window.localStorage.getItem(LOCALE_STORAGE_KEY)).toBe("de");
  });

  it("sends null for 'Browser default' and forgets the stored choice", async () => {
    mockedGet.mockResolvedValue({ locale: "de", translate_outbound: false });
    mockedPut.mockResolvedValue({ locale: null, translate_outbound: false });
    window.localStorage.setItem(LOCALE_STORAGE_KEY, "de");
    renderPage();

    const select = await screen.findByLabelText("Default language");
    fireEvent.change(select, { target: { value: "" } });

    await waitFor(() => {
      expect(mockedPut).toHaveBeenCalledWith({ locale: null });
    });
    await waitFor(() => {
      expect(window.localStorage.getItem(LOCALE_STORAGE_KEY)).toBeNull();
    });
  });

  it("surfaces the C38 invalid_locale rejection without changing the language", async () => {
    mockedGet.mockResolvedValue({ locale: null, translate_outbound: false });
    const err = new axios.AxiosError("bad request");
    // @ts-expect-error — minimal AxiosResponse stub for the 400 branch.
    err.response = { status: 400, data: { code: "invalid_locale" } };
    mockedPut.mockRejectedValue(err);
    renderPage();

    const select = await screen.findByLabelText("Default language");
    fireEvent.change(select, { target: { value: "ja" } });

    await waitFor(() => {
      expect(
        screen.getByText(/isn’t one feedbackmonk ships/i),
      ).toBeInTheDocument();
    });
    expect(i18n.language).toBe("en");
  });

  it("renders an accessible error state when the setting cannot be loaded", async () => {
    mockedGet.mockRejectedValue(new Error("boom"));
    renderPage();
    await waitFor(() => {
      expect(screen.getByRole("alert")).toBeInTheDocument();
    });
  });
});
