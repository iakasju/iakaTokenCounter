// Fenetre stub « A venir » ouverte par le hook analytics (D6). Aucune logique d'historique :
// on affiche seulement le compte cible, passe en parametre d'URL par le backend.

const params = new URLSearchParams(window.location.search);
const account = params.get("account");
const el = document.getElementById("account");
if (el) {
  el.textContent = account ? `Compte cible : ${account}` : "Compte non precise.";
}
