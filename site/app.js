/* GuitarCraft site.
 * - Installers come straight from the latest GitHub release (see RELEASE below), so publishing a release
 *   updates the site; the app updates itself from the same release.
 * - Six strings run behind the page; the cursor plucks them, and with sound on they ring (Karplus–Strong).
 * - Russian and English, picked from the browser and switchable. */
(function () {
  'use strict';
  const REPO = 'DimkaRapirka/GuitarCraft-Desktop';
  const API = 'https://api.github.com/repos/' + REPO + '/releases/latest';
  const RELEASES = 'https://github.com/' + REPO + '/releases/latest';
  const root = document.documentElement;
  const store = {
    get(k) { try { return localStorage.getItem(k); } catch (e) { return null; } },
    set(k, v) { try { localStorage.setItem(k, v); } catch (e) { /* storage may be blocked */ } },
  };
  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

  /* ------------------------------------------------------------------ text */
  const T = {
    ru: {
      'nav.features': 'Возможности', 'nav.studio': 'Студия', 'nav.workshop': 'Мастерская', 'nav.get': 'Скачать', 'nav.faq': 'Вопросы',
      'sound.on': 'Звук струн', 'sound.hint': 'Проведите курсором по струнам',
      'hero.title': 'Играй как на <i>живом</i> риге.',
      'hero.lead': 'Педали, комбик, кабинет и микрофоны, которые двигаются руками. Выглядит и звучит как настоящая техника, работает офлайн и открыто для всех.',
      'hero.all': 'Все платформы', 'hero.news': 'Что нового', 'meta.released': 'вышла', 'meta.platforms': 'платформы',
      'hero.caption': 'Студия: два микрофона, комната, пульт',
      'download.for': 'Скачать для {os}', 'download.any': 'Скачать установщик',
      's1.n': '01', 's1.title': 'Всё, что <i>слышно</i>, и всё, что видно.', 's1.p': 'Четыре вещи, ради которых GuitarCraft и делали. Листайте: картинка справа меняется вместе с текстом.',
      'f1.t': 'Цепочка как на живом педалборде', 'f1.p': 'Педали, усилитель, кабинет и рэк стоят в том же порядке, что и в железе. Режим «Вся цепочка» показывает всё на одном экране, и каждую ручку можно крутить прямо там.',
      'f1.l1': 'Перетаскивание блоков и «+» в любом месте', 'f1.l2': 'Каналы Clean, Crunch, Rhythm, Lead', 'f1.l3': 'Тюнер, метроном, A/B пресетов',
      'f2.t': 'Студия вместо слайдеров', 'f2.p': 'Два микрофона у кабинета и стереопара в комнате. Двигайте их мышью, микшируйте на пульте, выбирайте комнату от будки до каменной церкви.',
      'f2.l1': 'Позиция, дистанция, угол, фаза', 'f2.l2': 'Шесть комнат с настоящей акустикой', 'f2.l3': 'Свой IR для кабинета',
      'f3.t': 'Мастерская педалей', 'f3.p': 'Корпус четырёх размеров, баллончики с краской, покрытия, ручки, светодиоды, шильдик и свои рисунки. Каждая педаль выглядит так, как вы захотите.',
      'f3.l1': 'Свой рисунок наклейкой или во весь корпус', 'f3.l2': 'Промпт для генератора картинок', 'f3.l3': 'Свои педали из DSP‑блоков',
      'f4.t': 'Педалборд, который хочется показать', 'f4.p': 'Педали стоят на борде как настоящие: с джеками, футсвитчами и износом. Скины комбиков можно делать самому из одной картинки.',
      'f4.l1': 'Скины комбиков из PNG', 'f4.l2': 'Готовый промпт для ИИ‑генератора', 'f4.l3': 'Фоны сцены на свой вкус',
      's2.n': '02', 's2.title': 'Студия, а не <i>эффект</i>.', 's2.p': 'В AmpliTube микрофоны микшируют, а комнату добавляют ручкой. Здесь это настоящая комната: размеры, отражения, хвост, и микрофоны, которые стоят там, где вы их поставили.',
      'n1.t': 'Два микрофона', 'n1.p': 'Тащите их по диффузору: центр ярче, край мягче. Колесо — дистанция, Shift — угол.',
      'n2.t': 'Кабинеты', 'n2.p': 'Пять кабинетов от 1×10 до 4×12 или свой импульс.',
      'n3.t': 'Комнаты', 'n3.p': 'Отражения считаются по размерам комнаты, хвост — по её реверберации.',
      'n4.t': 'Пульт', 'n4.p': 'Фейдеры, панорама, фаза и соло для каждого микрофона и комнаты.',
      'room.booth': 'Будка', 'room.studio': 'Студия', 'room.live': 'Live‑комната', 'room.garage': 'Гараж', 'room.hall': 'Зал', 'room.church': 'Церковь',
      's3.n': '03', 's3.title': 'Звук, который <i>собирает агент</i>.', 's3.p': '',
      'ag.big': 'Скажите, какой звук нужен. Агент сам найдёт педали, послушает и поправит.',
      'ag.p': 'GuitarCraft умеет работать с ИИ‑агентами через MCP. Каждое действие видно в приложении и отменяется одной кнопкой. Доступ включается в настройках, по умолчанию выключен.',
      'ag.you1': 'Хочу тёплый овердрайв для блюза, и убери гул на низах', 'ag.ai1': 'Нашёл Verdant Drive, ставлю перед усилителем и включаю A/B.',
      'ag.ai2': 'Низ выше эталона на 6 дБ. Поднимаю срез НЧ до 120 Гц.', 'ag.ai3': 'Сохранил как «Тёплый блюз».',
      's4.n': '04', 's4.title': 'Мастерская', 's4.badge': 'скоро', 's4.p': 'Комбики, педали и рисунки от сообщества. Нашли скин, нажали «Установить», и он уже в вашем GuitarCraft. Пока здесь примеры.',
      'ws.install': 'Установить', 'ws.wait': 'Рисуется…', 'ws.by': 'от сообщества', 'ws.builtin': 'в комплекте',
      'ws.toast': 'Мастерская скоро откроется: скин «{name}» поставится в GuitarCraft одной кнопкой.',
      'ws.make': '<b>Сделайте свой.</b> В GuitarCraft есть готовый промпт: опишите стиль, сгенерируйте картинку в любом ИИ и загрузите PNG. Ручки встанут сами.',
      'ws.makebtn': 'Как сделать скин',
      'sk.crimson': 'Чёрный комбик с треснувшей маской и красным светом', 'sk.ironhead': 'Голова для тяжёлого звука', 'sk.midnight': 'Синий хай‑гейн', 'sk.seafoam': 'Сёрф‑комбо цвета морской волны', 'sk.walnut': 'Бутик в корпусе из ореха',
      's5.n': '05', 's5.title': 'Скачать. <i>Бесплатно.</i>', 's5.p': 'Установщики берутся из последнего релиза на GitHub, поэтому версия всегда свежая. GitHub‑аккаунт не нужен.',
      'os.mine': 'ваша система', 'os.download': 'Скачать',
      'os.mac.note': 'Apple Silicon (M1 и новее)', 'os.mac.how': 'Откройте .dmg и перетащите GuitarCraft в «Программы».',
      'os.win.note': 'Windows 10 / 11, 64‑bit', 'os.win.how': 'Запустите установщик, дальше всё само.',
      'os.linux.note': 'Debian / Ubuntu, 64‑bit', 'os.linux.how': 'Двойной клик по .deb или sudo apt install ./файл.deb',
      'auto': '<b>Обновляется само.</b> Установили один раз, дальше GuitarCraft сам находит и ставит новые версии. Подпись каждого обновления проверяется.',
      'sums': 'SHA‑256', 'notes': 'Список изменений',
      's6.n': '06', 's6.title': 'Вопросы',
      'q1': 'macOS пишет, что приложение нельзя открыть', 'a1': 'GuitarCraft пока не нотаризован в Apple, поэтому система просит подтвердить запуск. Перетащите его в «Программы», нажмите правой кнопкой → «Открыть» → «Открыть». Это нужно один раз. Если не помогло: <code>xattr -dr com.apple.quarantine /Applications/GuitarCraft.app</code>',
      'q2': 'Windows показывает «Windows защитила ваш компьютер»', 'a2': 'У установщика пока нет сертификата издателя, и SmartScreen осторожничает. Нажмите «Подробнее» → «Выполнить в любом случае». Это нужно один раз.',
      'q3': 'Что нужно, чтобы играть', 'a3': 'Гитара и аудиоинтерфейс (или любой вход с инструментальным уровнем). Без гитары можно послушать встроенные DI‑риффы. Задержка обычно 3–6 мс.',
      'q4': 'Это правда бесплатно?', 'a4': 'Да. Код открыт на GitHub, приложение работает офлайн, без аккаунта и рекламы.',
      'q5': 'Как обновлять', 'a5': 'Никак: GuitarCraft сам проверяет новые версии и ставит их после подтверждения. Можно и вручную скачать свежий установщик здесь.',
      'q7': 'Как сделать свой скин комбика', 'a7': 'В GuitarCraft выберите усилитель → кнопка скина под ним → «Создать своё оформление». Опишите стиль, скопируйте готовый промпт в любой генератор картинок (ChatGPT, Midjourney и т. п.), затем «Импорт PNG». Панель с ручками встанет на пустую полосу сверху, ручки можно подвинуть. Пример: <a href="/skins/crimson_mask.png" download style="color:var(--ice)">Crimson Mask, 1536×1024</a>.',
      'q6': 'Есть версия для Intel Mac?', 'a6': 'Пока установщик только для Apple Silicon (M1 и новее). Сборка для Intel в планах.',
      'outro': 'Подключайте <i>гитару</i>.', 'foot.src': 'Исходный код', 'foot.bug': 'Сообщить об ошибке', 'foot.free': 'Открытый код · бесплатно',
      'intro.sub': 'Гитарный риг для компьютера', 'intro.skip': 'Пропустить',
      'released.fmt': 'ru-RU', 'doc.title': 'GuitarCraft — гитарный риг для компьютера',
    },
    en: {
      'nav.features': 'Features', 'nav.studio': 'Studio', 'nav.workshop': 'Workshop', 'nav.get': 'Download', 'nav.faq': 'FAQ',
      'sound.on': 'String sound', 'sound.hint': 'Sweep the cursor across the strings',
      'hero.title': 'Play like it’s a <i>real</i> rig.',
      'hero.lead': 'Pedals, a combo, a cabinet and mics you move by hand. It looks and sounds like real gear, works offline and is open to everyone.',
      'hero.all': 'All platforms', 'hero.news': 'What’s new', 'meta.released': 'released', 'meta.platforms': 'platforms',
      'hero.caption': 'Studio: two mics, the room, a console',
      'download.for': 'Download for {os}', 'download.any': 'Download the installer',
      's1.n': '01', 's1.title': 'Everything you <i>hear</i>, and everything you see.', 's1.p': 'The four things GuitarCraft was made for. Scroll: the picture follows the text.',
      'f1.t': 'A chain like a real pedalboard', 'f1.p': 'Pedals, amp, cabinet and rack sit in the same order as hardware. The Whole chain view puts it all on one screen, and every knob works right there.',
      'f1.l1': 'Drag blocks, “+” anywhere', 'f1.l2': 'Clean, Crunch, Rhythm and Lead channels', 'f1.l3': 'Tuner, metronome, preset A/B',
      'f2.t': 'A studio instead of sliders', 'f2.p': 'Two close mics on the cab and a stereo pair in the room. Move them with the mouse, mix them on the console, pick a room from a booth to a stone church.',
      'f2.l1': 'Position, distance, angle, phase', 'f2.l2': 'Six rooms with real acoustics', 'f2.l3': 'Your own cabinet IR',
      'f3.t': 'A pedal workshop', 'f3.p': 'Four enclosure sizes, spray cans, finishes, knobs, LEDs, a label maker and your own artwork. Every pedal looks the way you want.',
      'f3.l1': 'Your artwork as a print or a full wrap', 'f3.l2': 'A prompt for image generators', 'f3.l3': 'Build pedals from DSP blocks',
      'f4.t': 'A board worth showing off', 'f4.p': 'Pedals sit on the board like the real thing: jacks, footswitches and wear. Combo skins come from a single picture you make yourself.',
      'f4.l1': 'Combo skins from a PNG', 'f4.l2': 'A ready prompt for AI generators', 'f4.l3': 'Scene backgrounds of your choice',
      's2.n': '02', 's2.title': 'A studio, not an <i>effect</i>.', 's2.p': 'Elsewhere you mix mics and add room with a knob. Here it is an actual room: size, reflections, tail, and mics that stand where you put them.',
      'n1.t': 'Two mics', 'n1.p': 'Drag them across the cone: the centre is brighter, the edge softer. Wheel for distance, Shift for angle.',
      'n2.t': 'Cabinets', 'n2.p': 'Five cabinets from 1×10 to 4×12, or your own impulse.',
      'n3.t': 'Rooms', 'n3.p': 'Reflections follow the room size, the tail follows its reverb time.',
      'n4.t': 'Console', 'n4.p': 'Faders, pan, phase and solo for each mic and the room.',
      'room.booth': 'Booth', 'room.studio': 'Studio', 'room.live': 'Live room', 'room.garage': 'Garage', 'room.hall': 'Hall', 'room.church': 'Church',
      's3.n': '03', 's3.title': 'A tone an <i>agent builds</i>.', 's3.p': '',
      'ag.big': 'Say what tone you want. An agent finds the pedals, listens and fixes it.',
      'ag.p': 'GuitarCraft works with AI agents over MCP. Every action shows up in the app and undoes with one click. Access is off until you turn it on in Settings.',
      'ag.you1': 'I want a warm blues overdrive, and get rid of the low hum', 'ag.ai1': 'Found Verdant Drive, putting it before the amp and turning on A/B.',
      'ag.ai2': 'Lows are 6 dB above the reference. Raising the low cut to 120 Hz.', 'ag.ai3': 'Saved as “Warm blues”.',
      's4.n': '04', 's4.title': 'Workshop', 's4.badge': 'soon', 's4.p': 'Combos, pedals and artwork from the community. Find a skin, press Install, and it is in your GuitarCraft. These are examples for now.',
      'ws.install': 'Install', 'ws.wait': 'Being painted…', 'ws.by': 'community', 'ws.builtin': 'built in',
      'ws.toast': 'The workshop opens soon: the “{name}” skin will install into GuitarCraft with one click.',
      'ws.make': '<b>Make your own.</b> GuitarCraft has a ready prompt: describe a style, generate the picture in any AI and load the PNG. The knobs place themselves.',
      'ws.makebtn': 'How to make a skin',
      'sk.crimson': 'A black combo with a cracked mask and red light', 'sk.ironhead': 'A head for heavy tones', 'sk.midnight': 'Blue high gain', 'sk.seafoam': 'A seafoam surf combo', 'sk.walnut': 'A boutique amp in walnut',
      's5.n': '05', 's5.title': 'Download. <i>Free.</i>', 's5.p': 'Installers come from the latest GitHub release, so the version is always fresh. No GitHub account needed.',
      'os.mine': 'your system', 'os.download': 'Download',
      'os.mac.note': 'Apple Silicon (M1 and later)', 'os.mac.how': 'Open the .dmg and drag GuitarCraft to Applications.',
      'os.win.note': 'Windows 10 / 11, 64-bit', 'os.win.how': 'Run the installer; it does the rest.',
      'os.linux.note': 'Debian / Ubuntu, 64-bit', 'os.linux.how': 'Double-click the .deb or sudo apt install ./file.deb',
      'auto': '<b>Updates itself.</b> Install once; GuitarCraft finds and installs new versions on its own. Every update’s signature is checked.',
      'sums': 'SHA-256', 'notes': 'Changelog',
      's6.n': '06', 's6.title': 'Questions',
      'q1': 'macOS says the app can’t be opened', 'a1': 'GuitarCraft isn’t notarized by Apple yet, so macOS asks you to confirm. Drag it to Applications, right-click → Open → Open. You only do this once. If that doesn’t help: <code>xattr -dr com.apple.quarantine /Applications/GuitarCraft.app</code>',
      'q2': 'Windows shows “Windows protected your PC”', 'a2': 'The installer has no publisher certificate yet, so SmartScreen is cautious. Click More info → Run anyway. You only do this once.',
      'q3': 'What do I need to play', 'a3': 'A guitar and an audio interface (or any instrument-level input). Without a guitar you can listen to the built-in DI riffs. Latency is usually 3–6 ms.',
      'q4': 'Is it really free?', 'a4': 'Yes. The code is open on GitHub, and the app works offline with no account and no ads.',
      'q5': 'How do I update', 'a5': 'You don’t have to: GuitarCraft checks for new versions and installs them once you confirm. You can also download the latest installer here.',
      'q7': 'How do I make my own combo skin', 'a7': 'In GuitarCraft select the amp → the skin button under it → Create a skin. Describe a style, copy the ready prompt into any image generator (ChatGPT, Midjourney…), then Import PNG. The knobs go on the empty strip at the top and can be moved. Example: <a href="/skins/crimson_mask.png" download style="color:var(--ice)">Crimson Mask, 1536×1024</a>.',
      'q6': 'Is there an Intel Mac version?', 'a6': 'For now the installer is Apple Silicon only (M1 and later). An Intel build is planned.',
      'outro': 'Plug in your <i>guitar</i>.', 'foot.src': 'Source code', 'foot.bug': 'Report a bug', 'foot.free': 'Open source · free',
      'intro.sub': 'A guitar rig for your computer', 'intro.skip': 'Skip',
      'released.fmt': 'en-GB', 'doc.title': 'GuitarCraft — a guitar rig for your computer',
    },
  };
  let lang = store.get('gc.lang') || ((navigator.language || 'ru').toLowerCase().startsWith('ru') ? 'ru' : 'en');
  const t = (k, vars) => {
    let s = (T[lang] && T[lang][k]) ?? T.ru[k] ?? k;
    if (vars) for (const [a, b] of Object.entries(vars)) s = s.replace('{' + a + '}', b);
    return s;
  };

  function translate() {
    root.lang = lang;
    document.title = t('doc.title');
    document.querySelectorAll('[data-t]').forEach((el) => { el.textContent = t(el.dataset.t); });
    document.querySelectorAll('[data-th]').forEach((el) => { el.innerHTML = t(el.dataset.th); });
    document.querySelectorAll('[data-lang]').forEach((b) => b.classList.toggle('on', b.dataset.lang === lang));
  }

  /* ------------------------------------------------------------------ release */
  const FALLBACK = {
    tag_name: 'v0.12.0', html_url: RELEASES, published_at: '2026-11-25T17:30:53Z',
    assets: [
      { name: 'GuitarCraft_0.12.0_aarch64.dmg', size: 66844997 },
      { name: 'GuitarCraft_0.12.0_x64-setup.exe', size: 61170972 },
      { name: 'GuitarCraft_0.12.0_amd64.deb', size: 62630960 },
    ],
  };
  const OS = {
    mac: { label: 'macOS', ext: /_aarch64\.dmg$/ },
    win: { label: 'Windows', ext: /_x64-setup\.exe$/ },
    linux: { label: 'Linux', ext: /_amd64\.deb$/ },
  };
  function detectOS() {
    const p = ((navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || '').toLowerCase();
    const u = (navigator.userAgent || '').toLowerCase();
    if (/iphone|ipad|android/.test(u)) return 'other';
    if (/win/.test(p) || /windows/.test(u)) return 'win';
    if (/mac/.test(p) || /mac os/.test(u)) return 'mac';
    if (/linux|x11/.test(p) || /linux|x11/.test(u)) return 'linux';
    return 'other';
  }
  const mine = detectOS();
  let release = FALLBACK;
  const ver = (r) => String((r && r.tag_name) || '').replace(/^v/, '').split('.').map((n) => parseInt(n, 10) || 0);
  const newer = (a, b) => { const x = ver(a), y = ver(b); for (let i = 0; i < 3; i++) { if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) > (y[i] || 0); } return false; };
  const newest = (...list) => list.filter((r) => r && r.tag_name).reduce((best, r) => (!best || newer(r, best) ? r : best), null);
  const getJSON = async (url, ms) => {
    const ctl = new AbortController(), timer = setTimeout(() => ctl.abort(), ms);
    try {
      const r = await fetch(url, { cache: 'no-cache', signal: ctl.signal, headers: { Accept: 'application/json' } });
      if (!r.ok) throw new Error(String(r.status));
      return await r.json();
    } finally { clearTimeout(timer); }
  };
  /* The page's own release.json is written by a workflow on every release and is served
     from the same host, so it does not depend on the GitHub API (rate limits, blocked ranges).
     The API is only a second opinion; the newest answer wins and a stale cache never beats it. */
  function cached() {
    try { const c = JSON.parse(store.get('gc.release.v2') || 'null'); return c && Date.now() - c.at < 6 * 3600 * 1000 ? c.release : null; } catch (e) { return null; }
  }
  async function fetchRelease() {
    const stale = newest(cached(), FALLBACK);
    const sources = [getJSON('release.json', 6000), getJSON(API, 6000)];
    const results = await Promise.all(sources.map((p) => p.catch(() => null)));
    const fresh = newest(...results);
    if (fresh) store.set('gc.release.v2', JSON.stringify({ at: Date.now(), release: fresh }));
    return fresh ? newest(fresh, stale) : stale;
  }
  const mb = (n) => (n ? Math.round(n / 1048576) + (lang === 'ru' ? ' МБ' : ' MB') : '');
  function asset(os) {
    const spec = OS[os]; if (!spec) return null;
    const a = (release.assets || []).find((x) => spec.ext.test(x.name));
    return a ? { name: a.name, size: a.size, url: a.browser_download_url || RELEASES.replace('/latest', '/latest/download/') + a.name } : null;
  }
  function applyRelease() {
    const v = String(release.tag_name || '').replace(/^v/, '');
    document.querySelectorAll('[data-version]').forEach((el) => { el.textContent = v; });
    document.querySelectorAll('[data-released]').forEach((el) => {
      try { el.textContent = new Date(release.published_at).toLocaleDateString(t('released.fmt'), { day: 'numeric', month: 'long', year: 'numeric' }); } catch (e) { /* ignore */ }
    });
    document.querySelectorAll('[data-notes]').forEach((el) => { el.href = release.html_url || RELEASES; });
    document.querySelectorAll('[data-sums]').forEach((el) => {
      const s = (release.assets || []).find((x) => x.name === 'SHA256SUMS.txt');
      el.href = s ? s.browser_download_url : (release.html_url || RELEASES);
    });
    document.querySelectorAll('.os-card').forEach((el) => {
      const os = el.dataset.os, a = asset(os);
      el.classList.toggle('is-mine', os === mine);
      el.querySelector('[data-dl]').href = a ? a.url : (release.html_url || RELEASES);
      el.querySelector('[data-size]').textContent = a ? mb(a.size) : '';
      el.querySelector('[data-file]').textContent = a ? a.name : '';
    });
    const pick = OS[mine] ? mine : null, a = pick && asset(pick);
    document.querySelectorAll('[data-dl-main]').forEach((b) => {
      b.href = a ? a.url : '#get';
      b.querySelector('[data-label]').textContent = pick ? t('download.for', { os: OS[pick].label }) : t('download.any');
      b.querySelector('[data-meta]').textContent = 'v' + v + (a ? ' · ' + mb(a.size) : '');
    });
  }

  /* ------------------------------------------------------------------ rendered blocks */
  const SKINS = [
    { img: 'assets/skin_crimson_mask_s.jpg', name: 'Crimson Mask', d: 'sk.crimson', feature: true, src: 'ws.by' },
    { img: 'assets/skin_ironhead.jpg', name: 'Iron Head', d: 'sk.ironhead', src: 'ws.builtin' },
    { img: 'assets/skin_midnight.jpg', name: 'Midnight', d: 'sk.midnight', src: 'ws.builtin' },
    { img: 'assets/skin_seafoam.jpg', name: 'Seafoam', d: 'sk.seafoam', src: 'ws.builtin' },
    { img: 'assets/skin_walnut.jpg', name: 'Walnut', d: 'sk.walnut', src: 'ws.builtin' },
  ];
  function renderWorkshop() {
    const el = document.getElementById('ws-grid'); if (!el) return;
    el.innerHTML = SKINS.map((s) => `<article class="ws-card${s.feature ? ' feature' : ''} rv">
      <div class="ws-img" data-wait="${esc(t('ws.wait'))}"><img src="${s.img}" alt="${esc(s.name)}" loading="lazy" onerror="this.parentNode.classList.add('missing')"></div>
      <div class="ws-meta"><h3>${esc(s.name)}</h3><span class="by">${esc(t(s.src))}</span><p>${esc(t(s.d))}</p>
      <button class="btn" type="button" data-install="${esc(s.name)}">${esc(t('ws.install'))}</button></div></article>`).join('')
      + `<div class="ws-make rv"><p>${t('ws.make')}</p><a class="btn" href="#faq-skin">${esc(t('ws.makebtn'))}</a></div>`;
    observeReveal(el);
  }
  document.addEventListener('click', (e) => {
    const b = e.target.closest('[data-install]');
    if (b) toast(t('ws.toast', { name: b.dataset.install }));
  });
  const toastEl = document.querySelector('.toast');
  function toast(msg) {
    if (!toastEl) return;
    toastEl.textContent = msg; toastEl.classList.add('show');
    clearTimeout(toast.h); toast.h = setTimeout(() => toastEl.classList.remove('show'), 3800);
  }

  /* ------------------------------------------------------------------ strings and sound */
  class Strings {
    constructor(canvas, opts) {
      this.c = canvas; this.g = canvas.getContext('2d');
      this.o = Object.assign({ n: 6, top: .2, bottom: .8, alpha: .5, glow: '#bfe3ff' }, opts);
      this.s = Array.from({ length: this.o.n }, (_, i) => ({ t: -99, a: 0, xp: .5, w: 6 + i * 1.3, th: 1.05 + i * .08 }));
      this.t0 = performance.now(); this.alive = true; this.onPluck = null;
      this.resize(); addEventListener('resize', () => this.resize());
      requestAnimationFrame(this.frame.bind(this));
    }
    resize() { const d = Math.min(devicePixelRatio || 1, 2); this.W = innerWidth; this.H = innerHeight; this.c.width = this.W * d; this.c.height = this.H * d; this.g.setTransform(d, 0, 0, d, 0, 0); }
    y(i) { return this.H * (this.o.top + (this.o.bottom - this.o.top) * i / (this.o.n - 1)); }
    pluck(i, xp, a) {
      const s = this.s[i]; s.t = (performance.now() - this.t0) / 1000; s.a = Math.min(a, 46); s.xp = Math.max(.06, Math.min(.94, xp));
      if (this.onPluck) this.onPluck(i, s.xp, Math.min(1, a / 40));
    }
    cross(x, y, lx, ly) { for (let i = 0; i < this.o.n; i++) { const sy = this.y(i); if ((ly - sy) * (y - sy) < 0) this.pluck(i, x / this.W, Math.hypot(x - lx, y - ly) * .55 + 8); } }
    frame() {
      if (!this.alive) return;
      const g = this.g, now = (performance.now() - this.t0) / 1000; g.clearRect(0, 0, this.W, this.H);
      for (let i = 0; i < this.o.n; i++) {
        const s = this.s[i], y0 = this.y(i), age = now - s.t, amp = s.a * Math.exp(-age * s.th), N = Math.max(60, Math.round(this.W / 14));
        g.beginPath();
        for (let k = 0; k <= N; k++) {
          const u = k / N; let d = 0;
          if (amp > .15) {
            for (let m = 1; m <= 4; m++) d += Math.sin(m * Math.PI * u) * Math.sin(m * Math.PI * s.xp) / (m * m) * Math.cos(s.w * m * age) * (m === 1 ? 1 : Math.exp(-age * m * .5));
            d *= amp * 2.2;
          }
          const x = u * this.W, y = y0 + d; k ? g.lineTo(x, y) : g.moveTo(x, y);
        }
        const lit = Math.min(1, amp / 14);
        g.lineWidth = 1 + i * .22; g.strokeStyle = `rgba(199,205,214,${this.o.alpha * (.55 + .45 * lit)})`;
        g.shadowColor = this.o.glow; g.shadowBlur = lit * 18; g.stroke(); g.shadowBlur = 0;
      }
      requestAnimationFrame(this.frame.bind(this));
    }
  }

  /* Karplus–Strong: each string is a short noise burst through a damped delay line. Top string is low E. */
  const NOTES = [82.41, 110.0, 146.83, 196.0, 246.94, 329.63];
  const sound = { on: false, ctx: null, bufs: [], last: [] };
  function makeBuffers() {
    const ctx = sound.ctx, sr = ctx.sampleRate;
    sound.bufs = NOTES.map((f) => {
      const len = Math.floor(sr * 2.2), b = ctx.createBuffer(1, len, sr), out = b.getChannelData(0);
      const n = Math.max(2, Math.round(sr / f)), line = new Float32Array(n);
      for (let i = 0; i < n; i++) line[i] = (Math.random() * 2 - 1) * (0.5 + 0.5 * Math.sin(Math.PI * i / n));
      let p = 0, prev = 0; const decay = 0.996 - f / 90000;
      for (let i = 0; i < len; i++) {
        const cur = line[p], next = line[(p + 1) % n], v = decay * 0.5 * (cur + next);
        line[p] = v; out[i] = cur * 0.6 + prev * 0.4; prev = cur; p = (p + 1) % n;
      }
      return b;
    });
    sound.last = NOTES.map(() => 0);
  }
  function play(i, xp, vel) {
    if (!sound.on || !sound.ctx || !sound.bufs[i]) return;
    const now = sound.ctx.currentTime;
    if (now - sound.last[i] < 0.07) return;
    sound.last[i] = now;
    const src = sound.ctx.createBufferSource(); src.buffer = sound.bufs[i];
    const gain = sound.ctx.createGain(); gain.gain.value = 0.12 + 0.32 * vel;
    const pan = sound.ctx.createStereoPanner ? sound.ctx.createStereoPanner() : null;
    const tone = sound.ctx.createBiquadFilter(); tone.type = 'lowpass'; tone.frequency.value = 2200 + 3200 * vel;
    src.connect(tone).connect(gain);
    if (pan) { pan.pan.value = (xp - 0.5) * 1.2; gain.connect(pan).connect(sound.master); } else gain.connect(sound.master);
    src.start();
  }
  function setSound(on) {
    sound.on = on;
    if (on && !sound.ctx) {
      const AC = window.AudioContext || window.webkitAudioContext;
      if (!AC) { sound.on = false; return; }
      sound.ctx = new AC();
      sound.master = sound.ctx.createGain(); sound.master.gain.value = 0.7;
      const comp = sound.ctx.createDynamicsCompressor(); sound.master.connect(comp).connect(sound.ctx.destination);
      makeBuffers();
    }
    if (on && sound.ctx.state === 'suspended') sound.ctx.resume();
    document.querySelectorAll('[data-sound]').forEach((b) => { b.classList.toggle('on', sound.on); b.setAttribute('aria-pressed', String(sound.on)); });
  }

  /* ------------------------------------------------------------------ scroll behaviour */
  let io;
  function observeReveal(scope) {
    if (!('IntersectionObserver' in window)) { (scope || document).querySelectorAll('.rv').forEach((e) => e.classList.add('in')); return; }
    io = io || new IntersectionObserver((es) => es.forEach((e) => { if (e.isIntersecting) { e.target.classList.add('in'); io.unobserve(e.target); } }), { rootMargin: '0px 0px -10% 0px' });
    (scope || document).querySelectorAll('.rv:not(.in)').forEach((e) => io.observe(e));
  }
  function story() {
    const steps = [...document.querySelectorAll('.step')], frames = [...document.querySelectorAll('.frames img')], label = document.querySelector('.frames .label');
    if (!steps.length || !('IntersectionObserver' in window)) { steps.forEach((s) => s.classList.add('on')); frames[0] && frames[0].classList.add('on'); return; }
    const show = (i) => {
      steps.forEach((s, k) => s.classList.toggle('on', k === i));
      frames.forEach((f, k) => f.classList.toggle('on', k === i));
      if (label) label.textContent = String(i + 1).padStart(2, '0') + ' / ' + String(steps.length).padStart(2, '0');
    };
    show(0);
    const o = new IntersectionObserver((es) => es.forEach((e) => { if (e.isIntersecting) show(steps.indexOf(e.target)); }), { rootMargin: '-45% 0px -45% 0px' });
    steps.forEach((s) => o.observe(s));
  }
  function pins() {
    const notes = [...document.querySelectorAll('.note')];
    document.querySelectorAll('.pin').forEach((p, i) => {
      const on = () => notes.forEach((n, k) => n.classList.toggle('on', k === i));
      p.addEventListener('mouseenter', on); p.addEventListener('focus', on); p.addEventListener('click', on);
    });
  }
  function index() {
    const links = [...document.querySelectorAll('[data-index] a')];
    const map = new Map(links.map((a) => [a.getAttribute('href').slice(1), a]));
    if (!('IntersectionObserver' in window)) return;
    const o = new IntersectionObserver((es) => es.forEach((e) => {
      if (e.isIntersecting) { links.forEach((l) => l.classList.remove('on')); const a = map.get(e.target.id); a && a.classList.add('on'); }
    }), { rootMargin: '-45% 0px -50% 0px' });
    map.forEach((_, id) => { const s = document.getElementById(id); s && o.observe(s); });
    const top = document.querySelector('.top');
    addEventListener('scroll', () => top.classList.toggle('scrolled', scrollY > 30), { passive: true });
  }

  /* ------------------------------------------------------------------ opening */
  function intro() {
    const el = document.getElementById('intro');
    const q = location.search, force = /[?&]intro=1/.test(q), off = /[?&]intro=0/.test(q);
    let seen = false; try { seen = !!sessionStorage.getItem('gc.intro'); } catch (e) { /* ignore */ }
    const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
    if (!el) { root.classList.add('ready'); return; }
    if (off || ((seen || reduced) && !force)) { el.remove(); root.classList.add('ready'); return; }
    const name = el.querySelector('.name');
    'GuitarCraft'.split('').forEach((ch, i) => { const s = document.createElement('span'); s.style.setProperty('--i', i); s.innerHTML = i >= 6 ? '<i>' + ch + '</i>' : ch; name.appendChild(s); });
    const strings = new Strings(el.querySelector('canvas'), { top: .3, bottom: .7, alpha: .75 });
    /* One quick strum across all six strings; the logo appears with it. */
    const timers = [];
    for (let k = 0; k < 6; k++) timers.push(setTimeout(() => strings.pluck(k, .2 + Math.random() * .6, 42), 30 + k * 45));
    root.classList.add('intro-on');
    requestAnimationFrame(() => requestAnimationFrame(() => root.classList.add('play')));
    let done = false;
    const finish = () => {
      if (done) return; done = true; timers.forEach(clearTimeout);
      try { sessionStorage.setItem('gc.intro', '1'); } catch (e) { /* ignore */ }
      root.classList.add('done');
      setTimeout(() => { strings.alive = false; el.remove(); root.classList.remove('intro-on', 'play'); root.classList.add('ready'); }, 500);
    };
    el.querySelector('[data-skip]').addEventListener('click', finish);
    addEventListener('keydown', (e) => { if (e.key === 'Escape' || e.key === 'Enter') finish(); }, { once: true });
    setTimeout(finish, 1500);
  }

  /* ------------------------------------------------------------------ start */
  function start() {
    release = newest(cached(), FALLBACK);
    translate(); renderWorkshop(); applyRelease();
    fetchRelease().then((r) => { release = r; applyRelease(); });
    document.querySelectorAll('[data-lang]').forEach((b) => b.addEventListener('click', () => {
      lang = b.dataset.lang; store.set('gc.lang', lang); translate(); renderWorkshop(); applyRelease();
    }));
    const bg = new Strings(document.getElementById('strings'), { top: .16, bottom: .9, alpha: .3 });
    bg.onPluck = play;
    let lx = 0, ly = 0;
    addEventListener('pointermove', (e) => { if (lx || ly) bg.cross(e.clientX, e.clientY, lx, ly); lx = e.clientX; ly = e.clientY; }, { passive: true });
    document.querySelectorAll('[data-sound]').forEach((b) => b.addEventListener('click', () => setSound(!sound.on)));
    observeReveal(); story(); pins(); index(); intro();
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start); else start();
})();
