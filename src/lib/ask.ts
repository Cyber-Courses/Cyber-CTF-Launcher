"use client";

import { useSyncExternalStore } from "react";

/**
 * A question a dialog mounted once (in the app shell) asks for code that awaits the answer: `ask`
 * opens it and resolves with `answer`'s value. A second question while one is open answers the
 * first with `dismissed`.
 */
export function createAsk<Question, Answer>(dismissed: Answer) {
  type Asked = { id: number; question: Question; resolve: (answer: Answer) => void };
  let current: Asked | null = null;
  let asked = 0;
  const listeners = new Set<() => void>();
  const emit = () => {
    listeners.forEach((l) => {
      l();
    });
  };
  const subscribe = (l: () => void) => {
    listeners.add(l);
    return () => listeners.delete(l);
  };

  function ask(question: Question): Promise<Answer> {
    current?.resolve(dismissed);
    // No dialog mounted to show it (a window whose root mounts none): dismissed at once, never a
    // question waiting forever. Regression (#383): the Settings window mounted none, so its
    // removals waited for an answer that never came, and every later question was a no.
    if (listeners.size === 0) {
      current = null;
      return Promise.resolve(dismissed);
    }
    return new Promise((resolve) => {
      current = { id: ++asked, question, resolve };
      emit();
    });
  }

  function answer(value: Answer) {
    const open = current;
    current = null;
    emit();
    open?.resolve(value);
  }

  /** The open question (its `id` differs for each one asked), or null. */
  function useAsked() {
    return useSyncExternalStore(
      subscribe,
      () => current,
      () => null,
    );
  }

  /** The open question, or null, outside React (a test's double answers it). */
  const pending = () => current;

  return { ask, answer, useAsked, subscribe, pending };
}
