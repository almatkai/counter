import { updateCommandMetadata, LocalStorage } from "@raycast/api";

export default async function Command({ launchContext }) {
  let counter = parseInt((await LocalStorage.getItem("counter")) ?? 0);
  if (!launchContext?.noUpdate) {
    counter--;
    await LocalStorage.setItem("counter", counter);
  }
  await updateCommandMetadata({ subtitle: `Value: ${counter}` });
}
