# Design system

Sources : `ui/src/index.css` (tokens, utilitaires `glass`), `ui/src/components/tv/`
(composants maison), `ui/src/nav/` (focus), `ui/src/shell/` (cadre),
`ui/src/lib/motion.ts` (ressorts).

## Intention : monochrome, la couleur vient des œuvres

Aucune couleur d'accent. L'interface est noire, blanche et translucide ;
**toute la couleur vient du contenu** : l'affiche sélectionnée remplit l'écran,
très floutée et assombrie (`shell/AmbientBackdrop.tsx`), et sa palette
(calculée en Rust) ajoute deux sources de lumière douces. Réglage :
*Appearance > Artwork colour in background*.

**Un seul élément est spectaculaire : le focus**, façon tvOS. La carte
focalisée se soulève (×1,1), projette une ombre profonde et accroche un reflet
spéculaire ; sous la souris elle s'incline vers le curseur (parallaxe). Boutons,
onglets et lignes de réglages focalisés passent **blanc sur noir**.

## Tokens

| Rôle | Token | Valeur |
|---|---|---|
| Fond | `--background` | `oklch(0.11 0 0)` (noir neutre) |
| Texte | `--foreground` / `--muted-foreground` | blanc / blanc 58 % |
| Verre | `--glass`, utilitaires `glass` / `glass-strong` | blanc 7 % + flou 36 px + saturation 1,6 + liseré intérieur |
| Adaptatif (runtime) | `--ambient-base`, `--ambient-accent`, `--ambient-strength` | depuis `palette()` (`app/src/images.rs`) |
| Mise en page | `--gutter`, `--poster-w`, `--thumb-w`, `--card-gap`, `--sidebar-w`, `--content-left`, `--page-top` | redéfinis en Flick Frame et en densité compacte |

**Contraste garanti** : `palette()` ramène la couleur de base à une luminance
de 7,5 % ; en plus, le fond ambiant est assombri (luminosité 0,62) et voilé.

*Frosted glass* désactivé (`data-blur="false"`) : les surfaces `glass`
deviennent opaques, sans `backdrop-filter` (GPU faibles, écrans 4K).

### Typographie

- **SF Pro** sur les systèmes Apple (`-apple-system`), **Inter** variable
  embarquée (axe de taille optique) ailleurs. Pile dans `--font-sans` /
  `--font-heading`. Aucune police distante (CSP stricte).
- Titres serrés (`letter-spacing: -0.022em`), texte courant `-0.011em`.
- Titres de page 2,75 rem, héros 3,5 rem (ou le logo de l'œuvre), rangées
  1,3125 rem, texte 0,9375–1,0625 rem.

### Formes et profondeur

- Le rayon suit le contenu : affiche 8 px, vignette 12 px, tuile 16 px,
  panneau 24–32 px, boutons et onglets en pilule.
- La profondeur vient du focus (élévation + ombre) et du verre, pas d'ombres
  grises systématiques.

### Mouvement (`lib/motion.ts`, Motion)

- `focusSpring` (focus, pressions), `panelSpring` (panneaux, dialogues),
  `pillSpring` (surbrillance qui glisse entre onglets via `layoutId`),
  `enter` (entrées de contenu, 450 ms), `ambientFade` (fond, 1,1 s).
- *Performance > Animations* à 0, ou `prefers-reduced-motion` : `MotionConfig`
  passe en `reducedMotion="always"`.

## Composants (`components/tv/`)

| Composant | Rôle | Notes |
|---|---|---|
| `Button` (primary / glass / ghost / danger ; sm, md, lg, icon, icon-lg) | Actions | primary = blanc ; focus = blanc + ×1,08 |
| `MediaCard` (poster 2:3 / thumb 16:9) | Titres, épisodes, reprises | inclinaison + reflet au pointeur, progression, vu |
| `Shelf` | Rangée horizontale | `FocusGroup` : revenir dans la rangée restaure le dernier élément |
| `Segmented` | Onglets / tri / versions | pilule de sélection glissante, focus blanc |
| `SettingsGroup`, `ToggleRow`, `SelectRow`, `SliderRow`, `LinkRow`, `InfoRow` | Réglages façon tvOS | gauche/droite modifie la valeur de la ligne focalisée : jamais de popup à la télécommande |
| `HeroBackdrop`, `TitleArt`, `MetaLine` | Héros d'accueil et de fiche | l'image passe sous la barre latérale et s'estompe par un flou progressif (smoothui) |
| `TvDialog` | Ajout de serveur | Radix (animate-ui) + frontière de focus ; Retour ferme |
| `TextField`, `Notice`, `Spinner`, `EmptyState`, `Panel`, `Facts`, `Pill` | Formulaires, états, faits | `Pill` n'affiche que ce que les métadonnées disent |

Les primitives shadcn (`components/ui/`) et animate-ui
(`components/animate-ui/`) sont la base (Radix, accessibilité) ; les
composants `tv/` les habillent pour la télécommande.

## Navigation par focus (`nav/`)

- **Norigin** tient l'arbre de focus (`useTv` pour une cible, `FocusGroup`
  pour un conteneur, `Screen` pour la racine d'une route) et la géométrie.
  Le focus DOM suit toujours le focus Norigin : Entrée/A cliquent les
  boutons natifs, les champs reçoivent la saisie.
- L'écoute clavier de Norigin est désactivée (elle bloque les flèches même
  en pause et ne prévient que la feuille focalisée). **`nav/input.ts`**
  convertit clavier, manette (Gamepad API) et télécommande en actions
  (`move`, `activate`, `back`, `playPause`, `seek`, `menu`) ; tout composant
  peut s'inscrire avec `onAction` (le plus récent est prioritaire).
- Modalité : les effets de focus ne s'affichent que si l'utilisateur pilote
  au clavier/manette (`useModality`) ; à la souris, c'est le survol.
- Retour : ferme le dialogue ou le panneau, sinon remonte l'historique. Sur un
  écran de premier niveau, il renvoie d'abord à la navigation (comme Menu sur
  tvOS). Revenir sur une page restaure l'élément focalisé et le défilement.
- Manette : D-pad/stick, A/B (inversables), Start, LB/RB ±10 s, View = Maxi
  Frame.

## Cadre

- **Bureau** : barre latérale flottante en verre ; les héros passent dessous.
- **Flick Frame** : barre d'onglets tvOS en haut, qui s'efface quand on
  descend dans le contenu et revient quand le focus remonte. Police racine
  `clamp(18px, 1.25vw, 40px)` (tout est en rem), plein écran, curseur masqué
  après 2,5 s.

### Barre de titre (`shell/TitleBar.tsx`)

- **Windows / Linux** : fenêtre sans décorations natives (`decorations:
  false`, `shadow: true` : Windows 11 garde coins arrondis et ombre). Une
  bande transparente en haut déplace la fenêtre (double-clic : agrandir) ;
  une capsule en verre porte réduire / agrandir / fermer (fermer rougit au
  survol).
- **macOS** : supprimer la barre de titre supprimerait aussi la forme native
  de la fenêtre (coins, ombre, redimensionnement). La barre est donc en
  *overlay* (`tauri.macos.conf.json`) : le contenu passe dessous et les vrais
  feux tricolores sont placés dans l'en-tête de la barre latérale
  (`trafficLightPosition`). Seule la bande de déplacement est dessinée.
- Masquée en Flick Frame (plein écran) ; dans le lecteur, elle suit les
  commandes.

### Bords de défilement

Là où du contenu continue au-delà du bord, il s'estompe — et seulement là :
une rangée pas encore défilée n'a pas de fondu à gauche.
- Zones internes (rangées, casting, onglets, sections des réglages, listes du
  lecteur) : `FocusGroup fade="x" | "y"` → masque dégradé
  (`scroll-fade-x/y`), longueur animée via `@property`. Taille réglable par
  `--fade-size`.
- Écran principal : pas de masque (il couperait le verre des panneaux du fond
  ambiant) ; un flou progressif et un léger voile se posent en haut quand on
  a défilé, en bas tant qu'il reste du contenu.

## Lecteur

Barre du bas : titre, barre de lecture (focus par défaut : gauche/droite
±10 s, Entrée pause, Haut ouvre le panneau), transport, volume, panneau
**Info / Audio / Sous-titres** (et Vidéo s'il y a plusieurs pistes). Les
commandes se masquent après 3,5 s de lecture ; toute touche les rappelle.

La WebView ne voit pas les pixels vidéo (calque natif dessous) : **pas de
vrai flou au-dessus de la vidéo**, uniquement des voiles sombres translucides.
C'est une contrainte technique documentée, pas un choix esthétique.
