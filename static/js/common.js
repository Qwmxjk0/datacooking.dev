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

  const NAV = [
    {
      group: { th: "งานไฟล์", en: "Files" },
      items: [
        { id: "home", href: "/", th: "หน้าแรก", en: "Home" },
        { id: "encoding", href: "/fix-encoding.html", th: "แก้ไฟล์เพี้ยน", en: "Fix text", icon: "/icons/encoding.svg" },
        { id: "shrink", href: "/csv-parquet.html", th: "ทำให้ไฟล์เล็กลง", en: "Make smaller", icon: "/icons/shrink.svg" },
      ],
    },
    {
      group: { th: "ของเล่นเน็ต", en: "Network" },
      items: [
        { id: "ip", href: "/my-ip.html", th: "ดู IP ของฉัน", en: "My IP", icon: "/icons/ip.svg" },
        { id: "subnet", href: "/subnet.html", th: "คำนวณ subnet", en: "Subnet", icon: "/icons/subnet.svg" },
      ],
    },
    {
      group: { th: "เครื่อง", en: "Server" },
      items: [
        { id: "ram", href: "/status.html", th: "ดู RAM", en: "RAM", icon: "/icons/ram.svg" },
        { id: "donate", href: "/donate.html", th: "คิวกำลังใจ", en: "Cheer queue", icon: "/icons/donate.svg" },
      ],
    },
  ];

  function fmtBytes(n) {
    const g = n / (1024 * 1024 * 1024);
    return g >= 1 ? g.toFixed(1) + " GB" : Math.round(n / 1024 / 1024) + " MB";
  }

  async function refreshStatus() {
    const cpuEl = document.getElementById("sbCpu");
    if (!cpuEl) return;
    try {
      const res = await fetch("/api/v1/status");
      const data = await res.json();
      const cpu = data.cpu_percent;
      cpuEl.textContent = cpu == null ? "—" : Math.round(cpu) + "%";
      const cpuBar = document.getElementById("sbCpuBar");
      if (cpuBar && cpu != null) cpuBar.style.width = Math.min(100, cpu) + "%";
      if (data.ram) {
        document.getElementById("sbRamPct").textContent = Math.round(data.ram.used_percent) + "%";
        document.getElementById("sbRamBar").style.width = Math.min(100, data.ram.used_percent) + "%";
        document.getElementById("sbMem").textContent =
          fmtBytes(data.ram.used_bytes) + " / " + fmtBytes(data.ram.total_bytes);
      }
    } catch {}
  }

  function mountChrome(activeId) {
    if (document.querySelector(".shell")) return;
    const lang = currentLang();
    const bar = document.createElement("div");
    bar.className = "statusbar";
    bar.innerHTML =
      '<span class="statpill">CPU <b id="sbCpu">—</b><span class="statbar"><i id="sbCpuBar"></i></span></span>' +
      '<span class="statpill">RAM <b id="sbRamPct">—</b><span class="statbar"><i id="sbRamBar"></i></span></span>' +
      '<span class="statpill">Memory <b id="sbMem">—</b></span>';

    const side = document.createElement("nav");
    side.className = "side";
    let html = '<a class="side-brand" href="/"><img src="/icon.svg" alt="">DataCooking</a><div class="side-nav">';
    NAV.forEach((g) => {
      html += "<h3>" + g.group[lang] + "</h3>";
      g.items.forEach((it) => {
        const cls = it.id === activeId ? ' class="on"' : "";
        const icon = it.icon ? '<img src="' + it.icon + '" alt="">' : "";
        html += "<a" + cls + ' href="' + it.href + '">' + icon + (it[lang] || it.th) + "</a>";
      });
    });
    html += "</div>";
    side.innerHTML = html;

    const main = document.createElement("div");
    main.className = "main";
    while (document.body.firstChild) main.appendChild(document.body.firstChild);

    const shell = document.createElement("div");
    shell.className = "shell";
    shell.appendChild(side);
    shell.appendChild(main);
    document.body.appendChild(bar);
    document.body.appendChild(shell);

    const badge = document.getElementById("healthBadge");
    if (badge) badge.classList.add("hidden");

    refreshStatus();
    setInterval(refreshStatus, 2000);
  }

  window.DC = { currentLang, applyI18n, bindLangToggle, pingHealth, mountChrome };
})();
