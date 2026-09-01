import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import { RenderTree, ValueTree, symbolHref } from "./InspectorApp";

afterEach(cleanup);

describe("generic rendering components", () => {
  it("renders reflected wrapper structure without knowing semantic product ids", () => {
    render(
      <ValueTree
        node={{
          kind: "record",
          type_name: "Binder<FnSig>",
          fields: [
            {
              name: "value",
              value: {
                kind: "sequence",
                type_name: "Vec<Ty>",
                items: [{ kind: "scalar", type_name: "usize", value: 1 }],
              },
            },
          ],
        }}
        navigateSymbol={() => {}}
      />,
    );

    expect(screen.getByText("Binder<FnSig>")).toBeVisible();
    expect(screen.getByText("Vec<Ty> [1]")).toBeVisible();
    expect(screen.getByText("usize")).toBeVisible();
  });

  it("routes a generic semantic reference through its canonical path", async () => {
    const user = userEvent.setup();
    let selected: string | null = null;
    render(
      <RenderTree
        node={{
          kind: "navigation",
          target: {
            path: "external/core/clone/Clone/clone",
            label: "core::clone::Clone::clone",
            presentation: { eyebrow: "External function", badges: [] },
          },
        }}
        navigateSymbol={(path) => {
          selected = path;
        }}
      />,
    );

    await user.click(screen.getByRole("button", { name: /core::clone::Clone::clone/ }));
    expect(selected).toBe("external/core/clone/Clone/clone");
  });

  it("renders terminal truncation without an unusable continuation control", () => {
    render(
      <ValueTree
        node={{ kind: "truncated", summary: "node budget exhausted" }}
        navigateSymbol={() => {}}
      />,
    );

    expect(screen.getByText("node budget exhausted")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Continue…" })).not.toBeInTheDocument();
  });

  it("renders an explicit marker for recursive stash reentry", () => {
    render(
      <ValueTree
        label="referent"
        node={{ kind: "cycle", identity: "stash-0-ptr:Ty:7" }}
        navigateSymbol={() => {}}
      />,
    );

    expect(screen.getByText("referent")).toBeVisible();
    expect(screen.getByText("cycle")).toBeVisible();
    expect(screen.getByText("↩ stash-0-ptr:Ty:7")).toBeVisible();
  });

  it("encodes canonical symbol paths as opaque URL components", () => {
    expect(symbolHref("local/example/impl Foo/bar", "typed-ir")).toBe(
      "/symbols/local%2Fexample%2Fimpl%20Foo%2Fbar/typed-ir",
    );
  });
});
