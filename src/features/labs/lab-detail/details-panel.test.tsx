// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { DetailsPanel } from "@/features/labs/lab-detail/details-panel";
import { containerMachine, upStatus } from "@/features/labs/labs-test-kit";
import { expectMaskedPassword } from "@/test/secret";

const PASSWORD = "wonderland-admin";

const status = upStatus(
  [
    containerMachine("jenkins", "running", { ports: [{ published: 51234, target: 8080, login: { user: "admin", password: PASSWORD } }] }),
    containerMachine("gitea", "running", { ports: [{ published: 51235, target: 3000 }] }),
  ],
  { url: "http://127.0.0.1:51234/" },
);

describe("DetailsPanel", () => {
  it("shows a service's login under its port, the password masked until Show, copied either way", async () => {
    const user = userEvent.setup();
    render(<DetailsPanel status={status} place={null} meta="" provision={null} />);
    expect(screen.getByText(":51234")).toBeTruthy();
    expect(screen.getByText("admin")).toBeTruthy();
    expect(screen.getAllByText("Sign in")).toHaveLength(1);
    await expectMaskedPassword(user, PASSWORD);
  });
});
