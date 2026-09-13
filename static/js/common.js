(() => {
  const KEY = "dc-lang";

  function currentLang() {
    const stored = localStorage.getItem(KEY);
    if (stored === "en" || stored === "th") return stored;
    return "th";
  }

  function applyI18n(dict) {
    const lang = currentLang();
    const table = dict[lang] || dict.th;
    document.documentElement.lang = lang;
    document.querySelectorAll("[data-i]").forEach((el) => {
      const value = table[el.getAttribute("data-i")];
      if (value != null) el.textContent = value;
    });
    document.querySelectorAll("[data-i-html]").forEach((el) => {
      const value = table[el.getAttribute("data-i-html")];
      if (value != null) el.innerHTML = value;
    });
    document.querySelectorAll("[data-lang-btn]").forEach((btn) => {
      btn.setAttribute("aria-pressed", btn.getAttribute("data-lang-btn") === lang ? "true" : "false");
      btn.classList.toggle("bg-amber-500", btn.getAttribute("data-lang-btn") === lang);
      btn.classList.toggle("text-black", btn.getAttribute("data-lang-btn") === lang);
      btn.classList.toggle("text-slate-400", btn.getAttribute("data-lang-btn") !== lang);
    });
    return lang;
  }

  function bindLangToggle(dict) {
    document.querySelectorAll("[data-lang-btn]").forEach((btn) => {
      btn.addEventListener("click", () => {
        localStorage.setItem(KEY, btn.getAttribute("data-lang-btn"));
        applyI18n(dict);
        window.dispatchEvent(new CustomEvent("dc-lang"));
      });
    });
  }

  async function pingHealth(labels) {
    const badge = document.getElementById("healthBadge");
    const dot = document.getElementById("healthDot");
    const label = document.getElementById("healthLabel");
    if (!badge) return;
    const lang = currentLang();
    const text = (labels && labels[lang]) || labels.th;
    try {
      const res = await fetch("/health");
      if (!res.ok) throw new Error("unhealthy");
      badge.className = "health health-ok";
      if (dot) dot.className = "dot";
      label.textContent = text.online;
    } catch {
      badge.className = "health health-bad";
      if (dot) dot.className = "dot";
      label.textContent = text.offline;
    }
  }

  window.DC = { currentLang, applyI18n, bindLangToggle, pingHealth };
})();
