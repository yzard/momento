import { lazy, Suspense } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import PageErrorBoundary from "../../../../src/frontend/components/common/PageErrorBoundary";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("PageErrorBoundary", () => {
  it("renders healthy content without a refresh prompt", () => {
    render(<PageErrorBoundary onReload={vi.fn()}>Admin AI</PageErrorBoundary>);
    expect(screen.getByText("Admin AI")).toBeTruthy();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it.each([
    "Failed to fetch dynamically imported module: /assets/Faces-old.js",
    "error loading dynamically imported module: /assets/Faces-old.js",
    "Importing a module script failed.",
  ])("handles stale lazy imports after navigation: %s", async (message) => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const onReload = vi.fn();
    const MissingPage = lazy(() => Promise.reject(new TypeError(message)));
    const { rerender } = render(
      <PageErrorBoundary onReload={onReload}>Admin AI</PageErrorBoundary>,
    );
    rerender(
      <PageErrorBoundary onReload={onReload}>
        <Suspense fallback="Loading">
          <MissingPage />
        </Suspense>
      </PageErrorBoundary>,
    );
    expect(
      await screen.findByText("Page files could not be loaded"),
    ).toBeTruthy();
    expect(onReload).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Refresh page" }));
    expect(onReload).toHaveBeenCalledTimes(1);
  });

  it("handles Vite preload failures and removes its listener on unmount", () => {
    const removeListener = vi.spyOn(window, "removeEventListener");
    const { unmount } = render(
      <PageErrorBoundary onReload={vi.fn()}>Admin AI</PageErrorBoundary>,
    );
    const event = new Event("vite:preloadError", { cancelable: true });
    fireEvent(window, event);
    expect(screen.getByText("Page files could not be loaded")).toBeTruthy();
    expect(event.defaultPrevented).toBe(false);
    unmount();
    expect(removeListener).toHaveBeenCalledWith(
      "vite:preloadError",
      expect.any(Function),
    );
  });

  it("does not label ordinary rendering errors as a version update", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    function BrokenPage(): never {
      throw new Error("Unexpected application state");
    }
    render(
      <PageErrorBoundary onReload={vi.fn()}>
        <BrokenPage />
      </PageErrorBoundary>,
    );
    expect(screen.getByText("This page encountered an error")).toBeTruthy();
    expect(screen.queryByText(/Momento may have been updated/)).toBeNull();
  });
});
