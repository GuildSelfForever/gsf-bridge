"use strict";

// withGlobalTauri = true : l'API est exposée sur window.__TAURI__ (pas de bundler).
const tauri = window.__TAURI__;
const $ = (id) => document.getElementById(id);

const ACTION_LABELS = {
  login: "Me connecter avec Discord",
  choose_wow_dir: "Choisir le dossier de WoW",
  update: "Mettre à jour",
};

/** Dernier état reçu du moteur. */
let status = null;
/** Connexion en cours (navigateur ouvert). */
let loggingIn = false;
/** Mise à jour trouvée (UpdateInfo) ou null. */
let update = null;
/** « Plus tard » choisi : on ne redemande pas avant le prochain lancement. */
let updateDeferred = false;
/** Téléchargement/installation en cours. */
let installing = false;
/** Recherche lancée pour une mise à jour obligatoire (une fois par lancement). */
let requiredCheckDone = false;

function fmtTime(epoch) {
  if (!epoch) return "";
  const d = new Date(epoch * 1000);
  const today = new Date().toDateString() === d.toDateString();
  return today
    ? d.toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" })
    : d.toLocaleString("fr-FR", { dateStyle: "short", timeStyle: "short" });
}

function showError(message) {
  const el = $("error");
  el.textContent = message || "";
  el.hidden = !message;
}

async function call(cmd, args) {
  showError("");
  try {
    return await tauri.core.invoke(cmd, args);
  } catch (err) {
    showError(String(err));
    throw err;
  }
}

function isMandatory() {
  return Boolean(status && status.update_required);
}

// --- Écran principal --------------------------------------------------------

function render(s) {
  status = s;
  const connected = s.connection.state === "connected";
  const wowFound = Boolean(s.wow.root) && s.wow.flavors.length > 0;

  // Phrase principale (calculée par le moteur).
  const h = loggingIn
    ? { tone: "busy", text: "Autorise le bridge dans ton navigateur…", action: null }
    : s.headline;
  $("dot").dataset.tone = h.tone;
  if ($("headline").textContent !== h.text) $("headline").textContent = h.text;

  // Ligne secondaire : niveau déduit de `user.roles` (contrat étape 1),
  // sauf pour une vigie (appareil validé, pas de compte).
  const isVigie = connected && s.connection.kind === "vigie";
  const isOfficerRole =
    connected && (s.connection.roles || []).some((r) => r === "officer" || r === "admin");
  const levelLabel = !connected
    ? "Non connecté"
    : isVigie
      ? "Vigie"
      : (isOfficerRole ? "Officier : " : "Membre : ") + s.connection.username;
  const facts = [levelLabel, wowFound ? "WoW trouvé" : "WoW introuvable", s.game_running ? "Jeu lancé" : "Jeu fermé"];
  $("facts").textContent = facts.join(" · ");

  // Bouton d'action de la phrase.
  const btn = $("action-btn");
  btn.hidden = !h.action;
  btn.disabled = loggingIn;
  if (h.action) {
    btn.textContent = ACTION_LABELS[h.action] || "";
    btn.dataset.action = h.action;
  }

  // « Envoyer maintenant » : seulement quand un envoi a du sens.
  $("sync-btn").hidden = !connected || !wowFound || isMandatory() || Boolean(h.action);
  $("sync-btn").disabled = s.busy;

  // Rappel discret d'une mise à jour remise à plus tard.
  const later = $("update-later-btn");
  later.hidden = !(update && updateDeferred && !isMandatory());
  if (update) later.textContent = "Installer la version " + update.version;

  // Réglages.
  $("version").textContent = s.version;
  $("wow-root").textContent = s.wow.root
    ? s.wow.root + (s.wow.manual ? " (choisi à la main)" : " (trouvé automatiquement)")
    : "Introuvable";
  $("auto-dir-btn").hidden = !s.wow.manual;
  $("account").textContent = connected
    ? isVigie
      ? "Connecté comme vigie (PC validé par les officiers)"
      : (isOfficerRole ? "Connecté en tant qu'officier : " : "Connecté en tant que membre : ") +
        s.connection.username
    : "Non connecté";
  $("logout-btn").hidden = !connected;
  $("discord-login-btn").disabled = loggingIn;

  // Journal (du plus récent au plus ancien).
  const log = $("log");
  log.replaceChildren();
  for (const e of s.log) {
    const li = document.createElement("li");
    li.className = e.level;
    const time = document.createElement("time");
    time.textContent = fmtTime(e.at);
    li.append(time, " " + e.message);
    log.append(li);
  }

  // Mise à jour obligatoire : chercher la nouvelle version tout de suite.
  if (isMandatory() && connected && !update && !requiredCheckDone) {
    requiredCheckDone = true;
    tauri.core.invoke("check_update").then(onUpdateFound).catch(() => {});
  }
  if (isMandatory() && update && !dialog().open) openUpdateDialog();
  if (dialog().open) renderUpdateDialog();
}

async function refresh() {
  render(await call("get_status"));
}

async function login() {
  loggingIn = true;
  if (status) render(status);
  try {
    await call("login");
  } catch (_) {
    // Message déjà affiché.
  } finally {
    loggingIn = false;
    refresh();
  }
}

async function onAction() {
  const action = $("action-btn").dataset.action;
  if (action === "login") return login();
  if (action === "choose_wow_dir") return call("choose_wow_dir").then(refresh, () => {});
  if (action === "update") {
    if (update) return openUpdateDialog();
    const info = await call("check_update").catch(() => null);
    if (info) onUpdateFound(info);
    else showError("Aucune nouvelle version n'est encore proposée par le site. Réessaie dans quelques minutes.");
  }
}

// --- Fenêtre de mise à jour -------------------------------------------------

const dialog = () => $("update-dialog");

function onUpdateFound(info) {
  if (!info) return;
  update = info;
  if (isMandatory() || !updateDeferred) openUpdateDialog();
  if (status) render(status);
}

function renderUpdateDialog() {
  const mandatory = isMandatory();
  $("update-title").textContent = mandatory ? "Mise à jour obligatoire" : "Nouvelle version disponible";
  $("update-body").textContent =
    "La version " + update.version + " est prête (tu as la " + update.current_version + ").";
  $("update-mandatory").hidden = !mandatory;
  const notes = $("update-notes");
  notes.textContent = update.notes || "";
  notes.hidden = !update.notes;
  $("update-later").hidden = mandatory || installing;
  $("update-now").disabled = installing;
  $("update-now").textContent = installing ? "Installation…" : "Mettre à jour maintenant";
}

function openUpdateDialog() {
  if (!update) return;
  const d = dialog();
  $("update-error").hidden = true;
  renderUpdateDialog();
  if (!d.open) d.showModal();
  $("update-now").focus();
}

function deferUpdate() {
  if (isMandatory() || installing) return;
  updateDeferred = true;
  dialog().close();
  if (status) render(status);
}

async function installUpdate() {
  installing = true;
  $("update-error").hidden = true;
  $("update-progress-wrap").hidden = false;
  $("update-progress").removeAttribute("value");
  $("update-progress-label").textContent = "Téléchargement…";
  renderUpdateDialog();
  try {
    await tauri.core.invoke("install_update");
    $("update-progress-label").textContent = "Installation terminée, redémarrage…";
    // Sous Windows l'installeur relance l'application lui-même.
    if (tauri.process && tauri.process.relaunch) await tauri.process.relaunch();
  } catch (err) {
    installing = false;
    $("update-progress-wrap").hidden = true;
    const el = $("update-error");
    el.textContent = "La mise à jour n'a pas pu se faire. Réessaie plus tard. (" + String(err) + ")";
    el.hidden = false;
    renderUpdateDialog();
    $("update-now").focus();
  }
}

function onProgress({ downloaded, total }) {
  const bar = $("update-progress");
  const label = $("update-progress-label");
  if (total) {
    const pct = Math.min(100, Math.round((downloaded / total) * 100));
    bar.value = pct;
    label.textContent = "Téléchargement : " + pct + " %";
  } else {
    bar.removeAttribute("value");
    label.textContent = "Téléchargement : " + (downloaded / 1048576).toFixed(1) + " Mo";
  }
}

/** Garde le focus clavier dans la fenêtre de mise à jour. */
function trapFocus(ev) {
  if (ev.key !== "Tab") return;
  const items = [...dialog().querySelectorAll("button")].filter((b) => !b.hidden && !b.disabled);
  if (items.length === 0) {
    ev.preventDefault();
    return;
  }
  const first = items[0];
  const last = items[items.length - 1];
  if (ev.shiftKey && document.activeElement === first) {
    ev.preventDefault();
    last.focus();
  } else if (!ev.shiftKey && document.activeElement === last) {
    ev.preventDefault();
    first.focus();
  } else if (!dialog().contains(document.activeElement)) {
    ev.preventDefault();
    first.focus();
  }
}

// --- Lancement au démarrage de l'ordinateur --------------------------------

async function loadAutostart() {
  const box = $("autostart");
  try {
    box.checked = await tauri.core.invoke("get_autostart");
    box.disabled = false;
  } catch (_) {
    box.disabled = true;
  }
}

async function toggleAutostart() {
  const box = $("autostart");
  const err = $("autostart-error");
  err.hidden = true;
  box.disabled = true;
  try {
    await tauri.core.invoke("set_autostart", { enabled: box.checked });
  } catch (e) {
    err.textContent = "Impossible de changer ce réglage. (" + String(e) + ")";
    err.hidden = false;
  }
  await loadAutostart();
}

// --- Écran d'accueil (premier lancement, contrat étape 1) -------------------

/** Conclut l'écran d'accueil (démarrage auto choisi) puis lance la connexion. */
async function finishWelcome() {
  const btn = $("welcome-connect-btn");
  const err = $("welcome-error");
  err.hidden = true;
  btn.disabled = true;
  try {
    await tauri.core.invoke("complete_welcome", { autostart: $("welcome-autostart").checked });
  } catch (e) {
    // Le réglage de démarrage automatique a échoué : on continue quand même,
    // l'écran d'accueil ne doit pas bloquer la connexion.
    err.textContent = "Le lancement au démarrage n'a pas pu être activé, mais tu peux continuer. (" + String(e) + ")";
    err.hidden = false;
  }
  $("welcome").hidden = true;
  $("app-body").hidden = false;
  btn.disabled = false;
  login();
}

/** Affiche l'écran d'accueil au tout premier lancement seulement. */
async function setupWelcome() {
  let needsWelcome = false;
  try {
    needsWelcome = await tauri.core.invoke("needs_welcome");
  } catch (_) {
    needsWelcome = false;
  }
  $("welcome").hidden = !needsWelcome;
  $("app-body").hidden = needsWelcome;
  $("welcome-repo-btn").addEventListener("click", () => tauri.core.invoke("open_repo").catch(() => {}));
  $("welcome-connect-btn").addEventListener("click", finishWelcome);
}

// --- Démarrage --------------------------------------------------------------

async function init() {
  if (!tauri) {
    $("headline").textContent = "Ouvre GSF Bridge depuis l'application (aperçu navigateur).";
    $("dot").dataset.tone = "warn";
    return;
  }

  await setupWelcome();

  await tauri.event.listen("gsf://status", (e) => render(e.payload));
  await tauri.event.listen("gsf://update", (e) => onUpdateFound(e.payload));
  await tauri.event.listen("gsf://update-progress", (e) => onProgress(e.payload));
  // Clic sur la notification de mise à jour ou « Ouvrir GSF Bridge ».
  await tauri.event.listen("gsf://open-update", () => {
    if (update && !installing) openUpdateDialog();
  });

  $("autostart").addEventListener("change", toggleAutostart);
  // Le réglage peut changer hors de l'application (Gestionnaire des tâches).
  $("settings").addEventListener("toggle", () => {
    if ($("settings").open) loadAutostart();
  });
  loadAutostart();
  $("action-btn").addEventListener("click", onAction);
  $("sync-btn").addEventListener("click", () => call("sync_now").catch(() => {}));
  $("update-later-btn").addEventListener("click", openUpdateDialog);
  $("logout-btn").addEventListener("click", () => call("logout").catch(() => {}));
  $("discord-login-btn").addEventListener("click", login);
  $("choose-dir-btn").addEventListener("click", () => call("choose_wow_dir").then(refresh, () => {}));
  $("auto-dir-btn").addEventListener("click", () => call("reset_wow_dir").then(refresh, () => {}));

  const d = dialog();
  d.addEventListener("keydown", trapFocus);
  // Échap = « Plus tard » (sans effet si la mise à jour est obligatoire).
  d.addEventListener("cancel", (ev) => {
    ev.preventDefault();
    deferUpdate();
  });
  $("update-later").addEventListener("click", deferUpdate);
  $("update-now").addEventListener("click", installUpdate);

  const s = await call("get_status");
  render(s);
  // « il y a 2 min » vieillit : relire l'état chaque minute.
  setInterval(() => {
    if (!document.hidden) tauri.core.invoke("get_status").then(render, () => {});
  }, 60 * 1000);
  // Vérification au démarrage (la boucle de fond vérifie aussi toutes les heures).
  if (s.connection.state === "connected") {
    tauri.core.invoke("check_update").then(onUpdateFound).catch(() => {});
  }
}

window.addEventListener("DOMContentLoaded", init);
