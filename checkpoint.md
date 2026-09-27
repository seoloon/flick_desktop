**Me:**
C'est pas mal pour une première version. Il faut surtout retravailler l'UI. Déjà, je n'aime pas trop les couleurs d'accent, pour être sur un vrai Apple-like, j'aurais mis monochrome, les couleurs arrivent par les dynamiques   
(couleurs cover, image cover flouté en fond etc...)                                                                                                                                                                               
je vais te donner tout ce qu'il te faut, carte en main pour que tu redesign complètement le frontend:                                                                                                                             
https://github.com/shadcn-ui/ui (inévitable, ShadCN UI, collection de composants de qualités)                                                                                                                                     
https://github.com/imskyleen/animate-ui (composants animés basé sur Radix, ShadCN, Base et bien plus encore, très Apple)                                                                                                          
https://github.com/educlopez/smoothui (librairie de composants)                                                                                                                                                                   
https://github.com/motiondivision/motion (librarie d'animations profesionnelles, et c'est ça que tu dois utiliser en priorité pour les animations)                                                                                
https://github.com/NoriginMedia/norigin-spatial-navigation (navigation tvOS-style, à intégrer, très important)

**You:**
[you worked, gathered ressources etc...]

I've hit the usage limit, so the redesign is paused partway, and the interface won't start right now: ui/src has no main.tsx yet, so pnpm desktop will fail. The Rust backend is untouched.

Done:
- Stack: the UI package now uses React 19, Tailwind v4, Motion, Norigin, TanStack Query/Virtual and Zustand instead of SolidJS.
- shadcn: initialized in ui/ with the Radix base, plus input, scroll-area, separator, skeleton, sonner, dropdown-menu, label, slider and badge.
- animate-ui: Radix tabs, switch, dialog, sheet, tooltip and progress, plus the tilt effect (for the tvOS focus parallax) and sliding-number.
- smoothui: progressive-blur.
- Kept: the typed IPC layer (ui/src/ipc, including the Rust-generated types) is back in ui/src, and the old Solid screens are in ui/src_old/ to port from.
- Norigin, read from its source: the arrow callback only reaches the focused element, not its parents, and pressing Enter never triggers the element's native click by itself. So interactive elements will be native buttons;    
  Enter and gamepad A will trigger their click, and the player will pause Norigin to handle seeking itself.

Design plan (not coded yet):
- Color: monochrome chrome; all color comes from the content. The selected item's backdrop fills the screen, heavily blurred and darkened; the extracted palette serves as a tint that keeps text readable.
- Focus: tvOS-style lift, parallax tilt and sheen with Motion springs; buttons and tabs turn white on black when focused.
- Font: the system's SF Pro on macOS, bundled Inter elsewhere.
- Layout: translucent sidebar on desktop, tvOS tab bar in Maxi Frame, a full-width featured carousel on Home, and a glass player bar with an Info / Audio / Subtitles panel.

Left to do:
1. Replace the Geist font shadcn installed with the SF Pro / Inter stack, set a dark-only neutral theme, and remove next-themes and the stray cn package.
2. Write the navigation setup (Norigin init, gamepad, back handling), the shell, the ambient backdrop, and the cards and rows.
3. Port the screens from src_old: Home, libraries, detail, search, player, settings, servers, admin, debug.
4. Typecheck, test in the app, delete src_old, and update docs/DESIGN_SYSTEM.md.

Tell me to continue once your limit resets and I'll pick up at step 1