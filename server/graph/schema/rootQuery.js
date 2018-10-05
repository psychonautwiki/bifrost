'use strict';

const features = require('../../util/features');

/* PLEBISCITE */

let plebisciteSchema = '';
let plebisciteRootQuery = '';

if (features.has('plebiscite')) {
    plebisciteSchema = `
        type ErowidMeta {
            erowidId: ID!

            gender: String
            published: String

            year: Int
            age: Int
            views: Int
        }

        type ErowidSubstanceInfo {
            amount: String
            method: String
            substance: String
            form: String
        }

        type Erowid {
            title: String
            author: String
            substance: String

            meta: ErowidMeta
            substanceInfo: [ErowidSubstanceInfo]

            erowidNotes: [String]
            pullQuotes: [String]

            body: String
        }
    `;

    plebisciteRootQuery = `
        erowid(
            substance: String
            # author

            limit: Int=50
            offset: Int=0
        ): [Erowid]
    `;
}

const RootQuery = `
type SubstanceClass {
    chemical: [String]
    psychoactive: [String]
}

type SubstanceTolerance {
    full: String
    half: String
    zero: String
}

interface RoaRange {
    min: Float
    max: Float
}

type SubstanceRoaRange implements RoaRange {
    min: Float
    max: Float
}

type SubstanceRoaDurationRange implements RoaRange {
    min: Float
    max: Float

    units: String
}

type SubstanceRoaDose {
    units: String

    threshold: Float
    heavy: Float

    common: SubstanceRoaRange
    light: SubstanceRoaRange
    strong: SubstanceRoaRange
}

type SubstanceRoaDuration {
    afterglow: SubstanceRoaDurationRange
    comeup: SubstanceRoaDurationRange
    duration: SubstanceRoaDurationRange
    offset: SubstanceRoaDurationRange
    onset: SubstanceRoaDurationRange
    peak: SubstanceRoaDurationRange
    total: SubstanceRoaDurationRange
}

type SubstanceRoa {
    name: String
    dose: SubstanceRoaDose
    duration: SubstanceRoaDuration
    bioavailability: SubstanceRoaRange
}

type SubstanceRoaTypes {
    oral: SubstanceRoa
    sublingual: SubstanceRoa
    buccal: SubstanceRoa
    insufflated: SubstanceRoa
    rectal: SubstanceRoa
    transdermal: SubstanceRoa
    subcutaneous: SubstanceRoa
    intramuscular: SubstanceRoa
    intravenous: SubstanceRoa
    smoked: SubstanceRoa
}

type SubstanceImage {
    thumb: String
    image: String
}

type Substance {
    name: String
    url: String

    featured: Boolean

	effects: [Effect]
	experiences: [Experience]

    class: SubstanceClass
    tolerance: SubstanceTolerance
    
    roa: SubstanceRoaTypes
    roas: [SubstanceRoa]

    summary: String
    images: [SubstanceImage]

    addictionPotential: String
    toxicity: [String]

    crossTolerances: [String]
    dangerousInteractions: [Substance]
}

type Effect {
    name: String
    url: String

	substances: [Substance]
	experiences: [Experience]
}

type Experience {
	substances: [Substance]
	effects: [Experience]
}

${plebisciteSchema}

type Query {
    substances(
        # Name of the effect you want the substances of
        effect: String

        # Name of the substance you are looking for
        query: String
        
        # Chemical class of the substances you are looking for (i.e. tryptamine)
        chemicalClass: String

        # Psychoactive class of the substances you are looking for (i.e. nootropic)
        psychoactiveClass: String

        limit: Int=10
        offset: Int=0
    ): [Substance]

    effects(
        # Name of the effect you are looking for
        effect: String

        # Name of the substance you want the effects of
    	substance: String

        limit: Int=50
        offset: Int=0
    ): [Substance] @deprecated(reason: "This node will be removed soon. In order to fetch effect related information, use the specific nodes \`substances_by_effect\` or \`effects_by_substance\` instead.")

    substances_by_effect(
        # Name of the effect you are looking for
        effect: [String]

        limit: Int=50
        offset: Int=0
    ): [Substance]

    effects_by_substance(
        # Name of the substance you want the effects of
    	substance: String

        limit: Int=50
        offset: Int=0
    ): [Effect]

    experiences(
    	substances_by_effect: String,
    	effects_by_substance: String,
    	substance: String
    ): [Experience]

    ${plebisciteRootQuery}
}
`;

module.exports = () => [RootQuery];
