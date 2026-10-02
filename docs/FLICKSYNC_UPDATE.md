# Intégrer le lien d'invitation FlickSync dans le client

Ce document est destiné au développement du client Flick. Il décrit ce que le serveur FlickSync fournit
désormais et ce que le client doit faire. La référence côté serveur est `src/invite.rs`; le format normatif est
aussi dans [flick-integration.md](flick-integration.md#invitation-link).

## 1. Ce qui change pour le client

Avant : l'utilisateur copiait l'URL du serveur et la clé `kid:server_id:secret` séparément.
Maintenant : il colle **un seul lien**, généré par le serveur :

```
flicksync://sync.example.com/?v=1&tls=1#k=bWFpbjpkZWZhdWx0OjAxMjM0NTY3ODlhYmNkZWYwMTIzNDU2Nzg5YWJjZGVmMDEyMzQ1Njc4OWFiY2RlZg
```

Le client en extrait : l'adresse du serveur, http ou https, et la clé de signature. Le reste du protocole (REST,
WebSocket, tokens JWT HS256, `aud=flicksync`) **ne change pas**. `/health` et `/ready` non plus.

## 2. Format (v=1)

```
flicksync://<host>[:<port>]/?v=1&tls=<0|1>#k=<clé>
```

| Élément | Règle |
|---|---|
| `<host>` | nom DNS, IPv4, ou IPv6 entre crochets (`[::1]`), en minuscules |
| `<port>` | optionnel, entier 1 à 65535 |
| chemin | toujours `/` (ou absent). Tout autre chemin : lien invalide |
| `v` | entier, requis. Le client **refuse** toute valeur autre que `1` (message « mettez le client à jour ») |
| `tls` | requis : `1` = `https`/`wss`, `0` = `http`/`ws`. Autre valeur : invalide |
| `k` (dans le **fragment**) | base64url **sans padding** (RFC 4648 §5) de la chaîne UTF-8 `kid:server_id:secret` |
| autres paramètres | **ignorés** (compatibilité future) |

La clé est dans le fragment (après `#`) pour ne jamais être envoyée en HTTP ni apparaître dans des logs de proxy.

### Algorithme de parsing

À écrire à la main, pas avec un parseur d'URL générique (schéma inconnu, gestion du fragment variable selon les
bibliothèques).

```rust
fn parse_invitation(s: &str) -> Result<Invitation, InviteError> {
    let s = s.trim();                                        // tolère espaces/retours à la ligne du copier-coller
    let rest = s.strip_prefix("flicksync://").ok_or(NotAnInvitation)?;
    let (before, fragment) = rest.split_once('#').ok_or(MissingKey)?;
    let (location, query) = before.split_once('?').unwrap_or((before, ""));
    let authority = match location.split_once('/') {
        Some((a, "")) => a,
        Some(_) => return Err(PathNotAllowed),
        None => location,
    };
    validate_authority(authority)?;                          // host + port optionnel, voir ci-dessus

    let param = |text: &str, name: &str| text.split('&')
        .filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v);

    if param(query, "v") != Some("1") { return Err(UnsupportedVersion); }
    let tls = match param(query, "tls") { Some("1") => true, Some("0") => false, _ => return Err(BadTls) };
    let key = base64url_no_pad_decode(param(fragment, "k").ok_or(MissingKey)?)?; // puis UTF-8
    let key = SigningKey::parse(&key)?;                      // coupe en 3 sur ':' (secret = le reste)
    Ok(Invitation { host: authority.to_lowercase(), tls, key })
}
```

Contraintes de `SigningKey::parse` (identiques côté serveur) : `kid` et `server_id` de 1 à 128 caractères parmi
`A-Za-z0-9 - _ . : @` ; `secret` d'au moins 32 caractères (le secret généré en fait 48, alphabet base64url, donc
sans `:`).

## 3. Dériver les URLs

| Usage | Valeur |
|---|---|
| Base API REST | `http(s)://<host>[:<port>]` selon `tls` |
| WebSocket | `ws(s)://<host>[:<port>]/api/v1/rooms/{room_id}/ws` |
| Diagnostic | `GET <base>/health` (200 = vivant), `GET <base>/ready` (200 = prêt, 503 = pas encore de clé chargée) |

Ne pas ajouter de slash final ni de chemin à la base.

## 4. Jetons

Rien de nouveau : le client signe lui-même ses JWT avec la clé du lien.

```
Header : { "alg": "HS256", "kid": "<kid>" }
Claims : { "sub": "<id utilisateur>", "server_id": "<server_id>", "aud": "flicksync",
           "name": "<nom affiché>", "perms": ["rooms:create","rooms:join","chat:send"],
           "iat": <now>, "exp": <now + 1h max> }
```

Le serveur auto-généré utilise `server_id = "default"`. Le `server_id` du jeton **doit** être celui du lien.
Durée de vie maximale d'un jeton : 24 h par défaut côté serveur, visez 1 h et renouvelez.

## 5. Parcours utilisateur recommandé

1. Écran « Ajouter un serveur de visionnage » : un seul champ « Collez votre lien d'invitation ».
2. Au collage : parser (section 2). Erreurs, avec message clair par cas :
    - pas `flicksync://` : « Ce n'est pas un lien d'invitation FlickSync. »
    - `v` inconnue : « Ce lien vient d'une version plus récente, mettez Flick à jour. »
    - clé absente ou illisible : « Lien incomplet, recopiez-le en entier. »
3. Vérifier la connexion avant d'enregistrer : `GET <base>/health`, puis `GET <base>/ready`. Distinguer
   « serveur injoignable » (réseau, mauvaise adresse, `tls` incorrect) de « serveur pas prêt » (503).
4. Enregistrer le lien parsé. Afficher seulement l'hôte dans l'interface, **jamais** le secret.
5. Option : scanner un QR code (`flicksync invite --qr` côté serveur produit le même lien, encodé en QR).

Si la connexion échoue avec une erreur d'authentification (`UNAUTHENTICATED`, « unknown signing key » ou
« invalid or expired token ») sur un serveur qui répondait avant : la clé a probablement été changée. Proposer
« Demandez un nouveau lien à l'administrateur du serveur » et permettre de recoller un lien.

## 6. Sécurité côté client

- Le lien **est** un secret (il contient la clé de signature de tous les jetons de ce serveur). Le stocker comme
  un mot de passe (trousseau du système), pas dans un fichier de configuration en clair ni dans la télémétrie.
- Ne jamais écrire le lien, la clé ou les jetons dans les logs, rapports d'erreur ou analytics. Masquer le
  fragment si une URL doit être affichée.
- Avertir si `tls=0` vers un hôte qui n'est pas local/réseau privé : le trafic et les jetons circuleraient en
  clair.
- Un même lien est partagé par tous les invités d'un serveur. Pour révoquer un accès, l'administrateur fait une
  rotation (section 7) et rediffuse un nouveau lien.

## 7. Rotation des clés

L'administrateur lance `flicksync invite --rotate` puis redémarre le service. Une nouvelle clé s'ajoute (nouveau
`kid` de la forme `k-xxxxxx`, même `server_id`) et **l'ancienne reste valide** : un client avec l'ancien lien
continue de fonctionner jusqu'au retrait de l'ancienne clé. Le client n'a rien de spécial à faire : il remplace
simplement le lien enregistré quand l'utilisateur en colle un nouveau, et signe avec le nouveau `kid`.

## 8. Vecteur de test

Le test du serveur (`invitation_example_is_stable` dans `src/invite.rs`) garantit ce couple. Le client doit le
parser à l'identique et reproduire le lien :

```
clé  : main:default:0123456789abcdef0123456789abcdef0123456789abcdef
hôte : sync.example.com   tls : 1
lien : flicksync://sync.example.com/?v=1&tls=1#k=bWFpbjpkZWZhdWx0OjAxMjM0NTY3ODlhYmNkZWYwMTIzNDU2Nzg5YWJjZGVmMDEyMzQ1Njc4OWFiY2RlZg
```

Cas à tester côté client (tous doivent être acceptés ou refusés comme indiqué) :

| Entrée | Résultat |
|---|---|
| lien ci-dessus entouré d'espaces ou d'un retour à la ligne | accepté |
| `flicksync://[::1]:8443/?v=1&tls=0#k=<clé valide>` | accepté, hôte `[::1]:8443` |
| `flicksync://a.example/?v=1&tls=1&futur=x#k=<clé>&autre=1` | accepté (paramètres inconnus ignorés) |
| `https://a.example` | refusé (pas le bon schéma) |
| sans `#k=` | refusé (clé absente) |
| `v=2` | refusé (version non supportée) |
| `tls=2` ou `tls` absent | refusé |
| chemin `/x/` | refusé |
| port `99999` ou `0` | refusé |
| `k` en base64 invalide, ou décodé sans 3 parties, ou secret de moins de 32 caractères | refusé |

## 9. Checklist d'implémentation

- [ ] Parseur de lien (section 2) et ses tests (section 8)
- [ ] Dérivation des URLs REST / WebSocket (section 3)
- [ ] Vérification `/health` puis `/ready` à l'ajout du serveur
- [ ] Stockage sécurisé du lien, aucun secret dans les logs
- [ ] Signature des jetons avec `kid` / `server_id` / `secret` du lien
- [ ] Messages d'erreur distincts (lien invalide, version inconnue, injoignable, pas prêt, clé changée)
- [ ] Remplacement du lien enregistré (rotation) et avertissement `tls=0` hors réseau local
