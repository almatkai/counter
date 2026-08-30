/// <reference types="@raycast/api">

/* 🚧 🚧 🚧
 * This file is auto-generated from the extension's manifest.
 * Do not modify manually. Instead, update the `package.json` file.
 * 🚧 🚧 🚧 */

/* eslint-disable @typescript-eslint/ban-types */

type ExtensionPreferences = {}

/** Preferences accessible in all the extension's commands */
declare type Preferences = ExtensionPreferences

declare namespace Preferences {
  /** Preferences accessible in the `increment-counter` command */
  export type IncrementCounter = ExtensionPreferences & {}
  /** Preferences accessible in the `decrement-counter` command */
  export type DecrementCounter = ExtensionPreferences & {}
  /** Preferences accessible in the `reset-counter` command */
  export type ResetCounter = ExtensionPreferences & {}
}

declare namespace Arguments {
  /** Arguments passed to the `increment-counter` command */
  export type IncrementCounter = {}
  /** Arguments passed to the `decrement-counter` command */
  export type DecrementCounter = {}
  /** Arguments passed to the `reset-counter` command */
  export type ResetCounter = {}
}
