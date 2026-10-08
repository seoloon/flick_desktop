// The Flick wordmark (app/icons/basic/flick-wordmark-flat.svg) as a
// component: the mark followed by the lettering, in the surrounding text
// colour. The mark fills the first MARK_WIDTH units, but the lettering is set
// tight against it (the l's foot starts under the top bar), so clipping to
// that width is not enough: hide the `wordmark-letters` group as well to show
// the mark alone (collapsed sidebar).
import { cn } from "@/lib/utils";

/** viewBox width and height of the wordmark, and width of its mark. */
export const WORDMARK_WIDTH = 1524.36;
export const WORDMARK_HEIGHT = 491.06;
export const MARK_WIDTH = 459.79;

export function FlickWordmark({ className, title }: { className?: string; title?: string }) {
  return (
    <svg
      viewBox={`0 -5 ${WORDMARK_WIDTH} ${WORDMARK_HEIGHT}`}
      fill="currentColor"
      className={cn("shrink-0", className)}
      role={title ? "img" : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
    >
      <polygon points="79.64,28.34 196.32,28.34 116.68,480.00 0.00,480.00" />
      <polygon points="211.38,28.34 459.79,28.34 441.21,133.73 192.79,133.73" />
      <polygon points="180.84,201.48 384.09,201.48 355.55,363.32 152.30,363.32 304.15,282.40" />
      <g className="wordmark-letters">
        <path d="M616.64 28.34 537.01 480.00H415.15L494.79 28.34Z" />
        <path d="M586.43 480.00 646.18 141.11H768.04L708.28 480.00ZM712.90 106.55Q688.04 106.55 673.35 90.17Q658.66 73.78 662.71 50.84Q666.83 27.43 687.27 11.22Q707.71 -5.00 732.57 -5.00Q757.34 -5.00 772.11 11.18Q786.88 27.37 782.75 50.83Q778.69 73.81 758.18 90.18Q737.67 106.55 712.90 106.55Z" />
        <path d="M919.72 486.06Q864.55 486.06 829.17 464.09Q793.79 442.11 779.95 402.85Q766.10 363.60 775.30 311.46Q784.49 259.32 812.18 220.07Q839.86 180.82 882.99 158.84Q926.12 136.86 981.29 136.86Q1016.45 136.86 1043.34 145.96Q1070.23 155.05 1088.33 171.87Q1106.43 188.70 1114.67 212.49Q1122.90 236.29 1120.75 265.69L1005.95 282.36Q1006.18 269.02 1003.88 258.87Q1001.58 248.72 996.75 241.74Q991.91 234.77 984.50 231.29Q977.08 227.80 967.07 227.80Q951.01 227.80 937.58 236.89Q924.16 245.99 914.38 264.48Q904.60 282.97 899.69 310.86Q894.82 338.44 898.02 357.23Q901.23 376.03 911.37 385.58Q921.50 395.12 937.57 395.12Q947.57 395.12 956.25 391.49Q964.92 387.85 972.39 380.73Q979.86 373.60 985.82 362.99Q991.79 352.38 996.01 338.74L1104.98 355.11Q1096.60 385.42 1079.93 409.52Q1063.25 433.62 1039.31 450.75Q1015.38 467.88 985.13 476.97Q954.88 486.06 919.72 486.06Z" />
        <path d="M1242.88 399.98 1268.43 255.08H1285.41L1387.35 141.11H1524.36L1355.42 318.74H1320.86ZM1119.65 480.00 1199.29 28.34H1321.14L1241.50 480.00ZM1328.80 480.00 1274.83 346.02 1369.53 259.32 1467.64 480.00Z" />
      </g>
    </svg>
  );
}
