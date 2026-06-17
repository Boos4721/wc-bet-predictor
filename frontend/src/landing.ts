// 落地页脚本:hero 概率条按 data-grow 填充(A→B 错峰)。
function grow(bar: HTMLElement | null): void {
  if (!bar) return;
  bar.querySelectorAll<HTMLElement>("[data-grow]").forEach((s) => {
    s.style.flexGrow = s.getAttribute("data-grow") || "0";
  });
}

const a = document.getElementById("vbarA");
const b = document.getElementById("vbarB");
requestAnimationFrame(() => {
  setTimeout(() => grow(a), 120);
  setTimeout(() => grow(b), 480);
});
