import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ResourceListPage } from "./resource-list";

describe("ResourceListPage", () => {
  it("renders the header, the list, and the aside", () => {
    render(
      <ResourceListPage header={<h1>List</h1>} aside={<p>Context</p>}>
        <ul>
          <li>Row</li>
        </ul>
      </ResourceListPage>,
    );
    expect(screen.getByRole("heading", { name: "List" })).toBeInTheDocument();
    expect(screen.getByRole("listitem")).toHaveTextContent("Row");
    expect(screen.getByRole("complementary")).toHaveTextContent("Context");
  });

  it("leaves the page background to the shell", () => {
    const { container } = render(<ResourceListPage header={null}>Rows</ResourceListPage>);
    expect(container.firstElementChild?.className).not.toMatch(/min-h-screen|bg-/);
  });
});
