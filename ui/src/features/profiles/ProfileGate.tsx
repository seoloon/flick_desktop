// At startup, with multi-user on and nobody picked (Rust resumed no
// profile), show the picker once.
import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef } from "react";
import { useLocation, useNavigate } from "react-router";
import { profilesQuery, visibleProfiles } from "@/lib/profiles";

export function ProfileGate() {
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const { data } = useQuery(profilesQuery);
  const done = useRef(false);
  useEffect(() => {
    if (!data || done.current) return;
    done.current = true;
    if (data.enabled && !data.active && visibleProfiles(data.profiles).length > 0 && pathname !== "/profiles") navigate("/profiles", { replace: true });
  }, [data, pathname, navigate]);
  return null;
}
