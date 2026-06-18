// GSAP-powered motion: Google-style reveals, count-ups, toasts and modals.

import { gsap } from "gsap";

/** Fade + rise a set of elements into view, staggered like Material lists. */
export function revealStagger(selector: string, delay = 0): void {
  const els = gsap.utils.toArray<HTMLElement>(selector);
  if (!els.length) return;
  gsap.fromTo(
    els,
    { y: 18, opacity: 0 },
    {
      y: 0,
      opacity: 1,
      duration: 0.55,
      ease: "power3.out",
      stagger: 0.07,
      delay,
    }
  );
}

/** Animate a number from its current value to `to`, formatting each frame. */
export function countUp(
  el: HTMLElement,
  to: number,
  format: (v: number) => string,
  duration = 1.1
): void {
  const state = { v: Number(el.dataset.value ?? 0) };
  el.dataset.value = String(to);
  gsap.to(state, {
    v: to,
    duration,
    ease: "power2.out",
    onUpdate: () => {
      el.textContent = format(state.v);
    },
  });
}

/** Subtle press feedback on a clicked element. */
export function pulse(el: HTMLElement): void {
  gsap.fromTo(
    el,
    { scale: 0.96 },
    { scale: 1, duration: 0.4, ease: "back.out(2.4)" }
  );
}

let toastHost: HTMLElement | null = null;

export function toast(message: string, kind: "success" | "error" | "info" = "info"): void {
  if (!toastHost) {
    toastHost = document.createElement("div");
    toastHost.className = "toast-host";
    document.body.appendChild(toastHost);
  }
  const el = document.createElement("div");
  el.className = `toast toast--${kind}`;
  el.innerHTML = `<span class="toast__dot"></span><span>${message}</span>`;
  toastHost.appendChild(el);

  gsap.fromTo(
    el,
    { x: 40, opacity: 0 },
    { x: 0, opacity: 1, duration: 0.4, ease: "power3.out" }
  );
  gsap.to(el, {
    x: 40,
    opacity: 0,
    delay: 3.2,
    duration: 0.4,
    ease: "power2.in",
    onComplete: () => el.remove(),
  });
}

/** Open a modal element with a scrim; returns a close function. */
export function openModal(content: HTMLElement): () => void {
  const scrim = document.createElement("div");
  scrim.className = "scrim";
  const wrap = document.createElement("div");
  wrap.className = "modal";
  wrap.appendChild(content);
  scrim.appendChild(wrap);
  document.body.appendChild(scrim);

  gsap.fromTo(scrim, { opacity: 0 }, { opacity: 1, duration: 0.25 });
  gsap.fromTo(
    wrap,
    { y: 24, scale: 0.96, opacity: 0 },
    { y: 0, scale: 1, opacity: 1, duration: 0.4, ease: "power3.out" }
  );

  const close = () => {
    gsap.to(wrap, { y: 16, scale: 0.97, opacity: 0, duration: 0.22, ease: "power2.in" });
    gsap.to(scrim, {
      opacity: 0,
      duration: 0.25,
      delay: 0.05,
      onComplete: () => scrim.remove(),
    });
  };
  scrim.addEventListener("click", (e) => {
    if (e.target === scrim) close();
  });
  return close;
}
