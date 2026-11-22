import { expect, test } from "vitest";
import { Chrome } from "@selenium-next/webdriver";

test("reports browser status", () => {
  using chrome = new Chrome({ headless: true });

  const status = chrome.getStatus();

  expect(status.ready).toBe(false);
  expect(status.message.length).toBeGreaterThan(0);
});

test("delivers the navigation request event", async () => {
  using chrome = new Chrome({ headless: true });

  const requestReceived = new Promise<string>((resolve) => {
    chrome.on("networkRequest", (request) => {
      if (request.url.includes("selenium.dev")) {
        resolve(request.url);
      }
    });
  });

  chrome.navigate("https://selenium.dev");
  const requestUrl = await requestReceived;

  using page = chrome.defaultPage;
  expect(page).toBeDefined();
  expect(requestUrl).toContain("selenium.dev");
}, 60_000);
