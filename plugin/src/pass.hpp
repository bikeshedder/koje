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

// Behind a shown overlay (or stash) Hyprland blurs what the workspace drew
// (`blur:special`), but its pass simplification still skipped what lies
// under the overlay's windows – the wallpaper showed there and the blur
// spread it around them. Added while a special workspace shows, this draws
// nothing and turns the simplification off for the frame.
class CKojeNoCullPassElement : public IPassElement {
  public:
    virtual ~CKojeNoCullPassElement() = default;

    virtual void draw(const CRegion& damage) {}
    virtual bool needsLiveBlur() {
        return false;
    }
    virtual bool needsPrecomputeBlur() {
        return false;
    }
    virtual bool disableSimplification() {
        return true;
    }
    virtual const char* passName() {
        return "CKojeNoCullPassElement";
    }
};
