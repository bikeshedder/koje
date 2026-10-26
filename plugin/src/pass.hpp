// The overview in Hyprland's render pass: drawn in place of the workspace.
#pragma once

#define WLR_USE_UNSTABLE

#include <hyprland/src/render/pass/PassElement.hpp>

class COverview;

class CKojeOverviewPassElement : public IPassElement {
  public:
    explicit CKojeOverviewPassElement(COverview* overview) : m_overview(overview) {}
    virtual ~CKojeOverviewPassElement() = default;

    virtual void                draw(const CRegion& damage);
    virtual bool                needsLiveBlur();
    virtual bool                needsPrecomputeBlur();
    virtual std::optional<CBox> boundingBox();
    virtual CRegion             opaqueRegion();

    virtual const char*         passName() {
        return "CKojeOverviewPassElement";
    }

  private:
    COverview* m_overview;
};
