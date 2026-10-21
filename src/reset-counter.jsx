import { LocalStorage } from "@raycast/api";

export default async function Command() {
  let counter = 0;
  await LocalStorage.setItem("counter", counter);

}
