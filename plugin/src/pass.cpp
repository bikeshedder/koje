#include "pass.hpp"
#include "overview.hpp"

#include <hyprland/src/helpers/Monitor.hpp>

void CKojeOverviewPassElement::draw(const CRegion& damage) {
    m_overview->draw();
}

bool CKojeOverviewPassElement::needsLiveBlur() {
    return false;
}

bool CKojeOverviewPassElement::needsPrecomputeBlur() {
    return false;
}

std::optional<CBox> CKojeOverviewPassElement::boundingBox() {
    const auto M = m_overview->monitor.lock();
    if (!M)
        return std::nullopt;
    return CBox{{}, M->m_size};
}

// Not opaque: the dimmed background fades in over the wallpaper.
CRegion CKojeOverviewPassElement::opaqueRegion() {
    return CRegion{};
}
