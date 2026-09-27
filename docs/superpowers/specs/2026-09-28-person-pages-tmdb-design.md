# Pages acteur avec TMDB — Design

> Date : 2026-09-28 · Statut : validé en discussion, en relecture

## 0. Intention

La fiche d'un titre liste sa distribution, mais on ne peut pas ouvrir un
acteur. On veut une **page personne** (acteur, réalisateur, scénariste) qui
montre :

- sa **biographie**, sa photo, ses dates, venant de **TMDB** (plus complet
  que Plex, harmonisé entre Jellyfin et Plex, et sans solliciter les
  serveurs pour des données qu'ils n'ont pas) ;
- ses titres **présents sur tous les serveurs** du profil actif, lisibles et
  fusionnés ;
- une rangée **« Also known for »** : ses titres connus absents des serveurs,
  non lisibles.

IMDb est écarté : pas d'API publique, le scraping est interdit par ses
conditions d'utilisation, ses jeux de données gratuits n'ont pas de
biographies.

Hors périmètre : pages pour les titres « Also known for » (pas de fiche),
bandes-annonces, récompenses, liens sortants vers TMDB/IMDb.

## 1. Source TMDB

### 1.1 Crate `oneshot-tmdb` (`crates/providers/tmdb`)

Client HTTP de l'API TMDB v3 (`https://api.themoviedb.org/3/`), via
`oneshot-net` (même politique réseau, masquage des secrets dans les logs) :

- `person(id, language)` → `GET person/{id}?language=…&append_to_response=combined_credits,external_ids` :
  nom, biographie, `birthday`, `deathday`, `place_of_birth`,
  `known_for_department`, `profile_path`, `popularity`, crédits (films et
  séries, avec personnage/métier, popularité, nombre de votes, affiche),
  identifiants externes (IMDb).
- `credits_of(title)` → `GET movie/{id}/credits` ou `GET tv/{id}/aggregate_credits`.
- `search_person(name)` → `GET search/person?query=…`.
- `check_key()` → `GET authentication` (bouton « Tester »).

**Clé** : deux formats acceptés et détectés — jeton de lecture v4 (JWT, en
en-tête `Authorization: Bearer …`) ou clé v3 (32 caractères hexadécimaux, en
paramètre `api_key`, ajouté à la liste de masquage de `oneshot_net::redact`).

**Langue** : `general.language` des réglages, sinon la langue de l'OS, au
format TMDB (`fr-FR`). Biographie vide dans cette langue → second appel en
`en-US` pour la biographie seulement.

### 1.2 Clé et réglages

- Réglages › **Metadata** (nouvelle section) : champ « TMDB API key »
  (masqué), bouton **Test**, état (« Connected » / message d'erreur),
  bouton **Remove**.
- Stockage : **trousseau de l'OS**, entrée `tmdb-key`. Jamais dans
  `settings.json`, jamais renvoyée à l'UI (l'UI ne reçoit que « une clé est
  enregistrée : oui/non »).
- Attribution obligatoire affichée dans cette section, avec le logo TMDB en
  blanc (monochrome) : « This product uses the TMDB API but is not endorsed
  or certified by TMDB. »

### 1.3 Cache

`MetadataCache` existant, clés `tmdb:person:{id}:{lang}`,
`tmdb:credits:{movie|tv}:{id}`, `tmdb:match:{server}:{person-key}` ;
TTL 7 jours. Une page déjà vue s'ouvre sans appel TMDB.

### 1.4 Images

Photos et affiches TMDB servies par `oneshot-img` (la WebView ne charge
aucune origine distante) : chemin `tmdb/<taille>/<fichier>` où taille ∈
{`w185`, `h632`, `w342`} et fichier correspond à `^[A-Za-z0-9_-]+\.(jpg|png)$`.
Rust ne récupère que `https://image.tmdb.org/t/p/<taille>/<fichier>` ;
toute autre forme → 400. Mise en cache disque comme les autres images.

## 2. Identifier la personne sur TMDB

Entrée : la personne cliquée (`Credit` : `ItemRef` serveur, nom, rôle) et le
titre d'où l'on vient (s'il est connu).

1. Si le titre d'origine a un identifiant TMDB (`external_ids.tmdb`) :
   distribution TMDB de ce titre → personne dont le nom normalisé
   (casse/accents/espaces, même règle que les profils) est égal → **id exact**
   (lève les homonymes).
2. Sinon, si la personne Jellyfin a un identifiant TMDB (`ProviderIds.Tmdb`,
   lu sur la fiche personne du serveur) → id.
3. Sinon : `search/person` par nom → premier résultat de nom égal, le plus
   populaire.
4. Aucun résultat → page sans TMDB (§4.3).

Le résultat est mis en cache par (serveur, personne).

## 3. « Sur tes serveurs »

Nouvelle méthode du contrat `MediaProvider` :
`person_items(name, hint: Option<ItemRef>) -> Result<Vec<MediaItem>>`
(défaut : `Unsupported`, laissé de côté sans erreur comme les favoris).

- **Jellyfin** : `hint` de ce serveur → `Items?PersonIds={key}&Recursive` ;
  sinon `Persons?searchTerm={name}` → nom normalisé égal → `PersonIds`.
  Films, séries, épisodes exclus (trop nombreux ; la série suffit).
- **Plex** : recherche d'acteur par nom sur le serveur puis filtre des
  bibliothèques par cet acteur (`actor=`). **Non vérifié** : l'API Plex est
  peu documentée ici ; un test « live » contre le vrai serveur le confirme
  avant de s'y fier. Repli si indisponible : filmographie TMDB recherchée
  titre par titre par identifiant externe dans les bibliothèques (au plus
  40 titres, 8 en parallèle).

`Catalog::person_items` : fan-out sur les serveurs du profil actif (délai
par serveur existant), fusion des doublons (`dedupe`), `Unsupported` ignoré.

**« Also known for »** : crédits TMDB triés par nombre de votes, sans les
titres déjà présents sur les serveurs (même identifiant TMDB ou IMDb qu'un
titre de « sur tes serveurs »), 20 au plus, sans doublons film/série.

## 4. Page personne

### 4.1 Route et accès

`/person/:ref?name=…&from=…` (`ref` = `ItemRef` de la personne sur le
serveur, `from` = titre d'origine, optionnel). Les membres de « Cast &
Crew » de la fiche d'un titre deviennent cliquables et y mènent.

### 4.2 Contenu

- En-tête : photo (TMDB `h632`, sinon image serveur, sinon initiales), nom,
  métier (`known_for_department`), naissance (date, âge ou âge au décès),
  lieu de naissance, décès.
- Biographie : repliée à 5 lignes, bouton « More » / « Less ».
- Rangées (Shelf existante) : « Movies » et « TV Shows » sur les serveurs
  (cartes normales, lisibles) ; « Also known for » (cartes affiche TMDB,
  focalisables, sans action).
- Fond ambiant : la photo de la personne, comme les œuvres.
- Chargement : les données serveur et TMDB arrivent séparément ; chaque
  partie s'affiche dès qu'elle est prête.

### 4.3 Sans clé TMDB ou TMDB injoignable

La page montre ce que savent les serveurs (photo et biographie Jellyfin quand
elles existent ; nom seul pour Plex) et les titres sur les serveurs. Sans
clé : ligne discrète « Add a TMDB key in Settings › Metadata for biographies. »
TMDB injoignable : pas de message bloquant, la partie TMDB est absente.

## 5. Sécurité

- Clé TMDB : trousseau uniquement, masquée dans les logs (`api_key=` et
  en-tête `Authorization`), jamais envoyée à la WebView.
- Proxy d'images restreint (§1.4) : pas d'URL arbitraire.
- Aucune donnée de l'utilisateur envoyée à TMDB hormis les identifiants de
  titres/personnes et les noms recherchés.

## 6. Tests

- `oneshot-tmdb` (wiremock) : fiche personne, crédits, recherche, repli de
  langue, clé v3 en paramètre et v4 en en-tête, clé invalide → erreur
  claire.
- Identification (§2) : correspondance par distribution du titre,
  homonymes, repli par recherche.
- `person_items` Jellyfin et Plex (wiremock), repli Plex.
- « Also known for » : exclusion par identifiants, tri, limite.
- Proxy d'images TMDB : formes acceptées et refusées.
- UI (vitest) : helpers purs (âge, découpage des rangées).
- Live (ignorés, manuels) : clé TMDB réelle ; recherche d'acteur sur le vrai
  serveur Plex.
