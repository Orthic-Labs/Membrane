---
title: ontology-embedding-survey
topic: surveys
arxiv: 2406.10964
source: https://arxiv.org/abs/2406.10964
pages: 20
source_pdf_sha256: 3743f538668b1c045dd8dacbc068c2e796cb85f972901a2db4819772e722c274
converted_by: pymupdf get_text
---
ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
1
Ontology Embedding: A Survey of Methods,
Applications and Resources
Jiaoyan Chen, Olga Mashkova∗, Fernando Zhapa-Camacho∗, Robert Hoehndorf, Yuan He and Ian Horrocks
Abstract—Ontologies are widely used for representing domain
knowledge and meta data, playing an increasingly important
role in Information Systems, the Semantic Web, Bioinformat-
ics and many other domains. However, logical reasoning that
ontologies can directly support are quite limited in learning,
approximation and prediction. One straightforward solution is to
integrate statistical analysis and machine learning. To this end,
automatically learning vector representation for knowledge of an
ontology i.e., ontology embedding has been widely investigated.
Numerous papers have been published on ontology embedding,
but a lack of systematic reviews hinders researchers from gaining
a comprehensive understanding of this field. To bridge this
gap, we write this survey paper, which first introduces different
kinds of semantics of ontologies and formally defines ontology
embedding as well as its property of faithfulness. Based on this,
it systematically categorizes and analyses a relatively complete
set of over 80 papers, according to the ontologies they aim at and
their technical solutions including geometric modeling, sequence
modeling and graph propagation. This survey also introduces
the applications of ontology embedding in ontology engineering,
machine learning augmentation and life sciences, presents a new
library mOWL and discusses the challenges and future directions.
Index Terms—Ontology, Ontology Embedding, Web Ontology
Language, Knowledge Graph, Representation Learning
I. INTRODUCTION
Ontologies are formal, explicit and shared representations
of knowledge within a domain, with definitions and axioms
for concepts, properties, relations and other types of entities
[1]. Ontologies have been a critical technology in Knowledge
Management, Information Systems, the Semantic Web, Nat-
ural Language Processing and Artificial Intelligence, playing
an increasingly important role in many fields such as Health-
care, Bioinformatics and E-commerce. A simple ontology
can just be a set of concepts arranged in a hierarchy with
the subsumption (inclusion) relationship between concepts
indicating that instances of one concept all belong to another.
Such ontologies are capable of representing taxonomies of
domains, such as the BBC Widelife Ontology [2] and the
International Classification of Diseases (ICD) [3], dating back
to Porphyrian Tree for presenting Aristotle’s categories in
the third century AD. Meanwhile, many data and knowledge
management systems such as e-commerce platforms [4] also
adopt such simple ontologies for type information of data.
Jiaoyan Chen is from University of Manchester & University of Oxford in
the UK. Fernado Zhapa-Camacho, Olga Mashkova and Robert Hoehndorf
are from King Abdullah University of Science and Technology in Saudi
Arabia. Yuan He and Ian Horrocks are from University of Oxford in the
UK. ∗indicates Olga Mashkova and Fernando Zhapa-Camacho make equal
contributions. More details of the author contributions are shown in the end.
With the fast development of the Web in 1990s, representing
and exchanging data and knowledge on the Web became
desirable. To this end, several standards were proposed for
defining more complex ontologies for constructing the Se-
mantic Web [5][6]. In 1999, Resource Description Framework
(RDF)1 which defines the syntax of triple (Subject, Predicate,
Object) was proposed for representing data, and in 2000,
the vocabulary of RDF Schema (RDFS)2, was proposed for
building ontologies as the schemas of data [7]. The vocabulary
of RDFS can represent not only hierarchical concepts, but also
instance membership to concepts, property hierarchies, and
property domains and ranges.
The Web Ontology Language (OWL), including its second
version OWL 2, was published upon the foundation of RDF
and RDFS for building ontologies that can represent more
complex knowledge with logics such as the disjunction, con-
junction and disjointness of concepts, and the existential and
universal rules3. OWL was underpinned by Description Logic
— a fragment of first-order logic with decidable reasoning
and efficient decision procedures [8]. Many widely used
ontologies, such as the Gene Ontology (GO) [9], the Food
Ontology (FoodOn) [10], the DBpedia ontology [11] and the
aforementioned BBC Widelife Ontology, adopt OWL [12].
Besides the formally and explicitly defined semantics, most
real-world ontologies are also equipped with literals, includ-
ing attribute values of instances defined by data properties,
and meta-data associated with entities4 defined by annotation
properties5, representing information of name, definition, com-
ment, image, version and so on. For example, the concept
obo:FOODON 00002873 in FoodOn is associated with an
English name “okara”, a synonym “soy pulp”, a long definition
“Okara, soy pulp, or tofu dregs is a pulp consisting of insoluble
parts of the soybean that ...”, the source of this definition, and
an image of okara [10]. These annotations, originally created
for human understanding, contain important information that is
often complementary to the formal semantics. However, they
cannot be utilised by symbolic reasoning.
In early 2010s, word embedding algorithms like Word2Vec
were proposed to represent natural language words as low
dimensional vectors, capturing their semantic relationships like
1https://www.w3.org/TR/rdf11-concepts/
2https://www.w3.org/TR/rdf-schema/
3https://www.w3.org/TR/owl-features/
4In the community of ontology, a concept is modeled as a class, an instance
corresponds to an individual in Description Logic, and the term “entity”
includes class, instance and property. In the community of knowledge graph,
the mention of an entity is actually equivalent to an instance. For clarity, we
adopt the terms of the ontology community in this paper.
5https://www.w3.org/TR/owl-ref/#Annotations
arXiv:2406.10964v3  [cs.AI]  7 Apr 2025


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
2
correlations within the vector space [13]. Similar ideas of
representation learning were also applied to knowledge graphs
(KGs) that are composed of relational facts, giving rise to some
classic algorithms like TransE [14] and RDF2Vec [15]. The
instances and relations (i.e., object properties) are embedded
with their semantics indicated by facts retained in the vector
space. Take TransE as an example; the embedding of a KG,
denoted as a mapping function v(·) from its instances and
relations to vectors, is learned such that each relational fact
(h, r, t) where the head instance h, the relation r and the tail
instance t correspond to the subject, predicate and object in
an RDF triple, respectively, is kept in the vector space as
v(h)+v(r) ≈v(t). Such embeddings not only enable machine
learning and statistical algorithms to utilise the knowledge, but
also support neural-symbolic reasoning within a KG, with both
approximation and prediction [16][17].
Similarly, representation learning has also been applied to
ontologies for embedding. Some early works such as [18]
and [19] proposed to embed triples with relations of tran-
sitivity, including those from WordNet [20]. Their methods
are applicable to simple ontologies with concept hierarchies.
However, the semantics of RDFS and OWL ontologies are
much more complex, and, accordingly, more advanced vector
representation models, which use high dimensional balls,
boxes and axis-aligned cones for representing concepts, were
recently proposed, following some early works including Em-
bedS [21] for RDFS ontologies, ELEmbeddings [22] and E2R
[23] for OWL ontologies of Description Logic EL++ and
ALC, respectively. Meanwhile, several complex embedding
frameworks such as OPA2Vec [24] and OWL2Vec* [25] were
also proposed to embed both formal semantics and textual
literals, often upon sequence learning methods.
Such ontology embedding methods have brought new solu-
tions for ontology engineering and dramatically extended the
application of ontologies. Many of them have been verified
in tasks with real-world data, including concept subsumption
inference [25], the domain application of protein–protein inter-
action prediction [24], and ontology augmented zero-shot and
few-shot learning [26][27]. Besides ontology embedding, there
are some other directions that attempt to combine ontology
with machine learning or statistical approach, including fuzzy
ontology standards such as Fuzzy OWL [28], traditional ontol-
ogy learning methods such as the inductive logic programming
system DL-Leaner [29][30], and some neural-symbolic frame-
works such as Logic Tensor Network [31]. In comparison with
them, ontology embedding methods focus on the automatic
learning of the given knowledge’s vector representations so as
to supporting the integration with different machine learning
and statistical models.
Briefly, there are quite a few results about ontology em-
beddings, covering theoretic analysis, new methods and ap-
plications. Although some survey papers for KG embedding
involve ontologies [32][33][34], but they only analyse those
embedding methods that aim at relational facts, regarding
simple ontologies as a kind of additional constraints. There is
a shortage of systematic review to papers on ontology embed-
dings. Kulmanov et al. [35] reviewed some works in 2021 on
using machine learning for analyzing semantic similarity with
ontologies. Several ontology embedding methods including
Onto2Vec [36], OPA2Vec [24] and EL Embeddings [22] are
covered, but they are far from complete, especially considering
there are quite a few papers published after 2021.
This survey aims to bridge the above gap, with (i) systematic
categorization and comparison of the ontology embedding
methods according to the employed techniques and the tar-
get ontologies, (ii) review of the applications in supporting
knowledge engineering, machine learning and life science
knowledge discovery together with benchmarks and metrics,
(iii) introduction and result demonstration of a library named
mOWL [37] that can support the implementation of ontology
embedding methods, and (iv) discussion on the challenges and
future directions. This survey has reviewed over 80 papers
(around 40 of them are for new embedding methods) published
in conferences and journals of Computer Science, AI and
Bioinformatics, covering all the relevant works on ontology
embedding, to the best of our knowledge. We believe it will
benefit all the researchers who are interested in some topics
among ontology, KG, knowledge representation, semantic em-
bedding, semantic techniques, knowledge engineering, neural-
symbolic integration, bioinformatics, and AI for life sciences.
The remainder of this paper is organized as follows. Section
II gives the background of ontology and semantic embedding.
Section III reviews ontology embedding methods. Sections
IV and V review the applications. Section VI demonstrates
mOWL. Section VII presents our perspectives on challenges
and future directions. Section VIII concludes the paper.
II. BACKGROUND
A. Symbolic Knowledge Representation with Ontologies
1) Knowledge Graph (KG): In this paper, we distinguish
KGs from ontologies. We refer to KGs as those knowledge
bases mainly composed of relational facts in RDF, following
the definition in most KG embedding papers [38][16]. A KG
can be denoted as G = {I, R, T }. I denotes a set of instances
(also known as entities), corresponding individuals in Descrip-
tion Logic. R denotes a set of binary relations. T denotes a set
of relational facts, i.e., T = {(h, r, t)|h, t ∈I, r ∈R}, where
(h, r, t) is an RDF triple, h and t, as the subject and object,
are also called as the triple’s head and tail, respectively. One
simple example is (Bob, hasFather, Alex). Sometimes (h, r, t)
is also denoted in form of a relation r(h, t). For real-world
KGs of the Semantic Web, each instance or relation should be
uniquely identified, usually by an Internationalized Resource
Identifier (IRI).
2) RDF Schema (RDFS): RDFS can define either an onto-
logical schema for a KG or an independent ontology with the
following main features.
• RDFS can define a set of concepts (classes) C and assert
the membership of instances using the built-in predicate
rdfs:type. For example, the triple (Alex, rdfs:type, Father)
represents that Alex is an instance of Father.
• RDFS can define the subsumption between concepts with
the built-in predicate rdfs:subClassOf. Considering two
concepts Father and Parent defined in C. The triple


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
3
(Father, rdfs:subClsasOf, Parent) represents that Parent sub-
sumes Father and Father is a sub-concept of Parent.
• RDFS can define the domain and range of a relation (i.e.,
object property) with the built-in predicates of rdfs:domain
and rdfs:range, respectively. If the domain (resp. range) of
a relation r is a concept C, the heads (resp. tails) that
are associated with r must be declared or inferred to be
instances of C. For example, we can define the range of
hasFather to Father, Parent and/or Male. RDFS can also
define the range of a data property with built-in data types.
• RDFS can define the subsumption between two properties
with the built-in predicate rdfs:subPropertyOf, indicating
that the instance pairs associated to one property all belong
to those associated to another property. One example is
(hasFather, rdfs:subPropertyOf, hasParent).
3) Description Logic (DL) and Web Ontology Language
(OWL): OWL has different sub-languages and comes in mul-
tiple versions. The complete languages, OWL Full and OWL
2 Full, are not decidable. In this part we mainly introduce (i)
the vocabularies of OWL 2 DL which are defined based on
the DL fragment SROIQ, and (ii) some of its widely used
sub-languages that are developed for different scenarios with a
better balance between knowledge expressivity and reasoning
complexity. A signature consists of three finite sets of symbols:
a set NI of individual names, a set NC of concept names,
and a set NR of role names. DL SROIQ allows recursive
concept definition as
⊤| ⊥| A | C ⊓D | C ⊔D | ¬C | ∃r.C | ∀r.C |
≥nr.C | ≤nr.C | ∃r.Self | {a}
where ⊤is the top concept, ⊥is the bottom concept, A ∈NC
is an atomic (or named) concept, r ∈NR is an atomic role
(equivalent to a binary relation), a ∈NI is an individual,
C and D are themselves (possibly complex) concepts, n is a
number of cardinality, ∃r.Self is a concept that indicates the set
of entities in the domain that are related by r with themselves
[8]. We say a concept is complex when it is constructed with
one or multiple logical operators such as ⊓, ⊔, ∃, ∀and ¬. A
DL ontology O can be composed of a TBox T and an ABox
A. The TBox defines logical background knowledge in the
form of concept subsumption axioms C ⊑D (Generalized
Concept Inclusion, GCI), and role axioms for logical back-
ground knowledge of role composition, role subsumption, and
role characteristics like functionality, transitivity and so on.
Sometimes these role axioms are separately divided into an
RBox, and accordingly the DL ontology is composed of a
TBox, an RBox and an ABox. The ABox contains concrete
data including concept assertions in form of C(a), and role
assertions in form of r(a, b). With the defined logic, symbolic
reasoners can be applied to infer hidden knowledge (i.e.,
entailment reasoning), check the ontology consistency and find
justification that leads to inconsistency.
ALC and EL++ are two important fragments of DL that are
widely investigated in ontology embedding. ALC is known as
Attributive Concept Language with Complements and allows
recursive concept definition with ⊤| ⊥| A | C ⊓D | C ⊔
D | ¬C | ∃r.C | ∀r.C [8]; ALC is a prototypical DL mainly
studied because it has most of the major features.
The DL fragment EL++ which corresponds to OWL 2 EL
profile allows recursive concept definition with ⊤| ⊥| A | C⊓
D | ∃r.C | {a} [39]. Due to a high knowledge representation
capability but a polynomial time complexity in reasoning, DL
EL++ is widely used by many real-world ontologies such as
SNOMED CT [40]. Example 1 presents a toy family ontology
of DL EL++, with a TBox and an ABox.
Example 1: The following EL++ ontology models a simple
family domain:
T = {Father ⊑Parent ⊓Male, Mother ⊑Parent ⊓Female,
Child ⊑∃hasParent.Father, Child ⊑∃hasParent.Mother,
hasParent ⊑relatedTo}
A = {Father(Alex), Child(Bob), hasParent(Bob, Alex)}
The ontology in Example 1 can be implemented with the
vocabularies defined in the standards of RDF, RDFS and
OWL; for example, rdfs:subClassOf for concept subsumption,
owl:ObjectSomeValuesFrom for the existential quantification
∃r.C and owl:ObjectAllValuesFrom for the universal quan-
tification ∀r.C. Figure 1 presents a fragment for the concept
obo:FoodON 00002809 (“edamame”) which is a sub-concept
of a named concept obo:FOODON 03304996 (“soybean sub-
stance”) and an existential quantification with the property
of obo:RO 0001000 (“derives from”) and the concept of
obo:FOODON 03411347 (“plant”).
obo:FOODON_03304996
(rdfs:label “soybean 
substance”)
obo:FOODON_00002809
(rdfs:label “edamame”)
rdfs:subClassOf
“Edamame is a preparation of immature soybean 
in their pods, or with the pod removed …”
obo:IAO_0000115
(rdfs:label “definition”)
ObjectSomeValuesFrom 
(obo:RO_0001000 (rdfs:label “derives from”),
obo:FOODON_03411347 (rdfs:label “plant”))
rdfs:subClassOf
“Commercially immature soybeans with or without pod 
are often marketed as Edamame in a frozen format.”
rdfs:comment
Fig. 1: A fragment from the OWL ontology FoodOn [10].
4) Ontology Literals: The literals of an ontology are mainly
defined in two approaches. (1) The first approach is to as-
sociate instances with literals by datatype properties, which
are sometimes known as attributes and whose values can be
of different types such as natural language phrases and long
text, real values, data and time, category, image and domain
specific sequence (e.g., gene sequence). For example, instances
of Person may have address, height, birth data and so on.
(2) The second approach is to associate entities with meta
information by annotation properties. In real-world ontologies,
most such literals are in form of text. For example, in the
ontology fragment in Figure 1, the concepts and properties are
annotated with names by the built-in vocabulary of rdfs:label6,
and obo:FOODON 00002809 is annotated with two long
sentences — a comment by rdfs:comment and a definition
6The textual value often has a language tag and is regarded as English by
default. The entity IRIs sometimes also indicate the name information.


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
4
by the ad-hoc annotation property obo:IAO 0000115. Some
other literals such as links to external sources, editors and
images are also widely used. Although all these literals include
important information, as informal semantics, they are often
ambiguous and cannot be used for inference by symbolic
ontology reasoners. In ontology embedding, ontologies either
with or without literals are considered.
5) Target Ontologies: In our context of ontology embed-
ding, “ontology” refers either to a TBox, or any DL ontol-
ogy with a non-empty TBox, i.e., (TBox, ABox, RBox) or
(TBox, ABox) with TBox̸ = ∅. Different embedding meth-
ods aim at different semantics of an ontology. Accordingly,
we divide the ontologies considered in the current ontology
embedding works into four kinds:
• Simple Ontology which refers to DL ontologies that have
only a TBox composed of the top concept ⊤(e.g., owl:Thing
in OWL ontologies), named concepts and subsumption
axioms between named concepts. They are equivalent to
taxonomies composed of hierarchical classes.
• Complex Ontology which refers to DL ontologies that have
a TBox containing any concept definitions of DL SROIQ
beyond ⊤. These ontologies may also contain an ABox, an
RBox, or both.
• Ontology with Literals which refers to simple or complex
ontologies that have literals.
• Ontology with KG which refers to KGs composed of large
scale relational facts, equipped with DL ontologies that has
a TBox (and an RBox in option) as the KG’s schema.
B. Semantic Embedding
In this part, we first summarise word embedding and KG
embedding, and then give formal definitions and properties of
general embedding and ontology embedding.
1) Word Embedding: Word embedding algorithms such as
Word2Vec [13] and GloVe [41] learn vector representations
of tokens (which are either words or sub-words) from a large
corpus concerning their semantics such as co-occurrence in the
sentences. Taking Word2Vec as an example, it learns a Feed-
Forward Neural Network from natural language sentences by
one of the two auto-encoding architectures — continuous skip-
gram which predicts the surrounding tokens of each token and
continuous Bag-of-Words which predicts a token based on its
surrounding tokens. For each token, the hidden layer output
of the network is its embedding. Tokens with more similar
meanings are expected to have higher vector similarities. Such
embedded semantics can partially support analogical reason-
ing, e.g., v(king) −v(father) ≈v(queen) −v(mother).
Embeddings by Word2Vec and GloVe are non-contextual,
which means each token has one unique vector representation
no matter what surrounding tokens it has. Recently, with the
development of Transformer-based encoder architectures [42]
and Pre-trained Language Models (PLMs) like BERT [43],
contextual word embeddings have been widely developed and
adopted. Taking BERT as an example, it learns a Transformer
architecture from a corpus by predicting the masked token
in each sentence and the next sentence of a given sentence.
The vector of a token is based on the attentions from itself
and its surrounding tokens in the sentence. Given a sentence
“the bank robber was seen on the river bank”, the first “bank”
and the second “bank” have different vectors due to different
surrounding tokens. Such contextual word embeddings encode
more semantics and often perform better than non-contextual
word embeddings in many tasks.
Besides natural language text, the above contextual and non-
contextual word embedding techniques are also applicable to
other kinds of sequential data, such as BioVec for biological
sequences like genes and proteins [44], and Node2Vec for
paths from graphs [45]. Their great success in many domains
also motivate researchers to applying them to KGs and on-
tologies. For simplicity, in this paper we call word embedding
as well as representation learning for other sequential data
as sequence learning.
2) Knowledge Graph Embedding: These methods learn
vector representations of instances and relations from the rela-
tional facts in a KG with their semantics retained in the vector
space [16]. In general, typical technical solutions can be di-
vided into geometric modeling such as the translational method
TransE [14], tensor decomposition such as the bilinear method
DistMult [46], neural network modeling such as ConvE [47],
random walk-based sequence modeling such as RDF2Vec [15].
TransE has been introduced in Section I. Taking RDF2Vec as
another example, it first conducts random walk for extracting
paths composed of instances and relations, and then learns a
Word2Vec model for encoding them. KG embedding has also
been extended to encode semantics beyond relational facts,
especially textual literals in combination with word embedding
methods [48][49], and logics such as horn rules, schemas and
constraints with additional modeling methods [32][34].
3) Formal Definitions of Embedding: Although the term
“embedding” has been widely used in contexts of machine
learning, NLP and KG, we find it useful to explicitly make
our understanding of embedding clear here as it will provide
a guide for analyzing and classifying ontology embedding
methods. We start with the definition of “embedding” as it
is used in mathematics:
Definition 1 (Embedding (mathematics)): An embedding
is an injective and structure-preserving map between two
mathematical structures which can be algebraic, topological,
or geometrical structures.
In machine learning, embedding is used in a somewhat differ-
ent sense:
Definition 2 (Embedding (machine learning)): An embed-
ding e is a learned mapping between the elements of a
mathematical structure and the elements of some structure S.
The notion of “embedding” in machine learning is therefore
related to, but less strict than, a structure-preserving map,
and embeddings are representations learned from data, largely
built upon the foundation of representation learning [50]. The
learned representations are usually not arbitrary but rather aim
to preserve some “semantics” of the original data. We capture
this by defining the notion of “faithfulness”:
Definition 3 (Faithfulness of embedding (machine learn-
ing)): An embedding (machine learning) e is “faithful” if e
converges to some embedding (mathematics) f.


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
5
4) Definitions and Properties of Ontology Embedding: An
ontology has a syntactic structure which consists of a set of
logical symbols like connectives and quantifiers, a set of non-
logical symbols including constants, functions and relations, a
set of well-formed formulas constructed from the logical and
non-logical symbols, a set of axioms (a subset of the well-
formed formulas), and a set of inference rules for deriving new
formulas from the axioms and previously derived formulas.
The signature of the ontology includes concept, individual
and relation symbols. The ontology’s model structure over its
signature consists of a non-empty set D, called the domain or
universe of the model, an interpretation function I that assigns
each individual symbol to an element of D, each concept
symbol to a subset of D, and each binary relation symbol
to a binary relation on D (i.e., a subset of D2). The semantic
structure of the ontology O, denoted as Mod(O), consists of
the collection of all model structures that satisfy the axioms
in O. Mod(O) can be seen as a class or a category in the
sense of category theory, depending on the context. With these
background, we formally define ontology embedding and its
faithfulness:
Definition 4 (Ontology embedding): Let O be an ontology
with signature ΣO. An ontology embedding is an embedding
(machine learning) e of the term algebra T(ΣO) generated by
the signature ΣO and the well-formedness rules of the under-
lying Description Logic for constructing concept descriptions,
role expressions, and axioms.
Definition 5 (Faithful ontology embedding): An embedding
of an ontology O is faithful if it converges to an embedding
(mathematics) which preserves some mathematical structure S
of O.
Here, we only consider embeddings for specific ontologies
(with a specific signature), not embeddings of an entire logic.
The notion of “faithfulness” for embeddings requires speci-
fying a mathematical structure S to which the embeddings
are faithful. There are at least three mathematical structures
that can be assigned to ontologies: (1) the syntactic structure,
potentially combined with syntactic inference rules (i.e., the
deductive calculus); (2) a single arbitrary model of the on-
tology, i.e., a model structure in which all ontology axioms
are true; and (3) the semantic structure of the ontology [51].
The mathematical structure that the embedding aims to pre-
serve can also be used to classify and distinguish ontology
embedding models.
Another property of ontology embedding is “interpretabil-
ity”. In machine learning, there is no universally-agreed
definition of interpretability [52]; in [53], interpretability is
described as “the ability to explain or to present in under-
standable terms to a human”. In the context of ontology em-
bedding, we can strictly define “interpretability” as the ability
to reconstruct both symbols and composition rules from their
embeddings. Faithful ontology embeddings are interpretable
in that sense since injectivity ensures that each symbol can
be uniquely recovered from its embedded representation, and
the preservation of a mathematical structure of an ontology
inherent in faithful embeddings allows for the restoration of the
composition rules between symbols. A non-faithful ontology
embedding does not support full restoration, but may still
allow restoring a part of the symbolic semantics and justifying
the inference with the embeddings. This characteristic is
important for assessing ontology embedding methods since
it reflects to what extent humans, who inherently engage
in symbolic reasoning, can read off ontology axioms and
understand the reasoning from the constructed embedding.
III. ONTOLOGY EMBEDDING METHODS
In this section, we first analyse the main technical solutions
that have been commonly adopted for ontology embedding
(Section III-A), and then review the ontology embedding
works for each kind of ontologies (Section III-B to III-E).
A. General Technical Solutions
The technical solutions that are commonly adopted by the
current ontology embedding methods include the following:
• Geometric Modeling aims to generate embeddings that
are faithful to some model structures of an ontology; they
generate (or approximate) logical models of an ontology
by interpreting concepts as geometric regions, interpret
individuals as members of these regions, and relations as
pairs of points standing in some geometric relation. Take
the embedding algorithms ELBE [54] and Box2EL [55] for
the DL EL++ as an example. They model an instance as a
high dimensional point, i.e., one single vector, and models a
concept as a high dimensional axis-aligned box represented
by one vector for the box center and one vector for the
box offset. The instantiation relation between an instance
and a concept is modeled as the instance’s point lying
within the concept’s box. The basic idea is demonstrated in
Figure 2 with a toy example. These types of embeddings are
highly interpretable since they induce geometric relations
between geometric objects representing relations, concepts
and individuals which align with ontology axioms.
• Sequence Modeling first transforms the ontology axioms
and literals into sequences composed of entities (and to-
kens), and then adopts a sequence learning model to learn
their embeddings. The basic idea is demonstrated in Figure
3. Take OWL2Vec∗[25] as an example. It first extracts
sequences composed of entities and textual literals from
an OWL ontology by mapping the ontology to a graph
and performing multiple random walks over the graph, and
then learns a word embedding model from the sequences
for encoding the entities and tokens in the text. These
methods are usually partially faithful to correlations and co-
occurrences of symbols in the axioms of the ontology, and
thus they have low interpretability.
• Graph Propagation represents an ontology by a (multi-
relation) graph with initial node representations, and then
learns a graph propagation model for new node represen-
tations. The basic idea is also demonstrated in Figure 3.
For example, for concept matching, the work [56] uses
two Graph Convolutional Networks to learn embeddings of
two cross-ontology concepts, respectively, by propagating
the initial word embeddings of their surrounding concepts.
Methods of this type are commonly interpretable to a limited
degree since they preserve mainly the concept hierarchy.


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
6
Solution
Pros
Cons
Targeted Ontologies and Citations
Geometric
Modeling
High interpretability; high
faithfulness to the semantic
structure
Hard to integrate
literals; not support
many features of OWL
Simple Onto.: [57][58][19][59][60][61][18] [62][63][64][65]
Complex Onto.: [66][22][23][67][68] [54][69][70][71][72][55]
Onto. & Lit.: [26]
Onto. & KG: [21][73][74][75][76][77][78]
Sequence
Modeling
Extensible to different
ontologies; support literals;
learn correlations as
effective features for
downstream tasks
Low interpretability;
only partial faithful to
correlations and
co-occurrences
Complex Onto.: [79][36][80]
Onto. & Lit.: [81][82][83][24][84] [85] [25][86][87][88][89][90][91]
Graph
Propagation
Well embed the graph
especially the concept
hierarchy
Low interpretability;
only partially faithful to
the graph structure
Simple Onto.: [92]
Onto. & Lit.: [93][56]
TABLE I: Important characteristics (pros and cons) of the three technical solutions, and the ontologies that main methods of each technical solution aim
at. Onto. and Lit. are short for ontology and literal, respectively.
Table I gives a summary of these technical solutions,
including an analysis of the pros and cons according to two
dimensions — faithfulness with regard to different mathe-
matical structures of an ontology, and interpretability which
indicates how transparency the reasoning in the vector space is.
Table I also classifies different ontology embedding methods
of each technical solution based on the ontologies that they
can deal with (i.e., the targeted ontologies).
𝐹𝑎𝑡ℎ𝑒𝑟 ⊑𝑀𝑎𝑙𝑒
𝐹𝑎𝑡ℎ𝑒𝑟 ≡𝑀𝑎𝑙𝑒⊓𝑃𝑎𝑟𝑒𝑛𝑡
𝐹𝑎𝑡ℎ𝑒𝑟(𝐴𝑙𝑒𝑥)
. . .
Ontology 
(Formal Semantics) 
Box-based Embeddings 
(2-dimension vector space) 
Parent
Male
Father
(Alex)
Optimisation by 
retaining the  
relationships
Fig. 2: Demonstration of the ontology embedding solution of
geometric modeling with the examples of ELBE and Box2EL.
B. Embedding Simple Ontology
The concept hierarchies of an ontology imply its basic struc-
tural semantics. We refer to ontology embedding that solely
consider concept hierarchies as simple ontology embedding
as it omits more complex logical semantics. Figure 4 presents
two dimensions — Embedding Method and Embedding Space,
their values and corresponding works of simple ontology
embedding.
Geometric modeling is the most prevalent technique for
simple ontology embedding. This includes box embedding
in Euclidean space, hyperbolic distance-based embedding in
hyperbolic space, cone embedding for both geometries, and
graph propagation in hyperbolic space.
In the context of Euclidean space, a key approach is to
devise a function that preserves the hierarchical order of
entities. A typical work by [57] proposes using the reversed
product order, essentially forming a Euclidean cone where
each entity’s embedding value is at least that of its parent
entity. Building on this, [58] extends to encode entities using
probability densities instead of points. The box embedding is
another typical construction which simulates the hierarchical
ordering with hyper-rectangles, where a child entity’s box is
consumed within its parent entity’s box. Unlike cone embed-
dings, which are typically parameterized by their apex, box
embeddings require two vectors representing the minimal and
maximal coordinates in the hyper-rectangle. To further im-
prove box embeddings, [59] explores a probabilistic relaxation
to achieve a smoother distribution, [60] investigates encoding
dual hierarchical relationships (hypernym and meronym) at the
same time, and [61] addresses the local identifiability issue,
proposing the Gumbel-box process as a solution.
Hyperbolic space, with its expansive property and theo-
retical underpinning, is particularly suitable for representing
hierarchical structures. The Poincar´e embedding [18] is a
typical approach that minimizes hyperbolic distances between
related entities while maximizing the separation from unre-
lated ones in a unit Poincar´e ball. This spatial arrangement
places more general entities near the origin and more specific
entities closer to the boundary, reflecting their hierarchical
depth. However, numerical instabilities near the boundary
of the manifold are a known challenge. To address this,
[94] investigates an alternative hyperbolic model, the Lorenz
(or Hyperboloid) model, while [65] proposes the extended
Poincar´e ball with geometric distortion. To augment the
embedded semantics, [63] and [64] explores the integration
of Poincar´e embeddings with pre-trained word embeddings,
while [92] utilises the hyperbolic graph convolutional networks
(HGCN) [95] for aggregating neighbourhood information. In
hyperbolic space, establishing geometric shapes like boxes and
cones, which are straightforward in Euclidean space, requires
more nuanced considerations. A notable contribution in this
line is the hyperbolic entailment cone by [62], which not
only preserves transitivity but also enables direct prediction
of entity subsumptions through its construction.
The techniques discussed here aim to achieve high geomet-
ric interpretability in encoding hierarchies, whether preserving
order or exploiting geometric properties. Although not all cited
works specifically test with ontology concept hierarchies, their
methodologies are readily applicable to such structures.
C. Embedding Complex Ontology
Complex ontologies are mainly embedded by geometric
modeling. The methods aim to map concepts, individuals
and roles into a continuous vector space (e.g., Rn) where


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
7
𝐹𝑎𝑡ℎ𝑒𝑟 ⊑𝑀𝑎𝑙𝑒
𝐹𝑎𝑡ℎ𝑒𝑟 ≡𝑀𝑎𝑙𝑒⊓𝑃𝑎𝑟𝑒𝑛𝑡
𝐹𝑎𝑡ℎ𝑒𝑟(𝐴𝑙𝑒𝑥)
. . .
Ontology 
Father
Parent
Male
Alex
“Alexander 
Hamilton”
rdfs:label
rdf:type
rdfs:subClassOf
(Alex, rdfs:label, “Alexander” …)
(Alex, rdf:type, Father, …)
(Father, EquivalentTo,  …)
. . .
Entity/Token 
Embeddings
Sequence 
Learning
Sequences
Graph to Sequences 
(e.g., Random Walk)
Axiom to
sequence
(e.g., syntax-
based 
serialisation)
Ontology to 
Graph
Entity 
Embeddings
Graph 
Propagation Model 
(e.g., GNN)
Fig. 3: Demonstration of the ontology embedding solutions of sequence modeling and graph propagation.
Embedding
Simple Ontology
Embedding Method
Box embedding [19][59][60][61]
Cone embedding [57][58][62]
Hyperbolic distance-based embedding [18][63][65][64]
Graph propagation [92]
Embedding Space
Euclidean space [57][58][19][59][60][61]
Hyperbolic space [18][62][63][65][64][92]
Fig. 4: Dimensions, their values and corresponding works of embedding simple ontology.
they can be represented as points or geometrical regions.
This allows to capture some aspects of semantics of the
underlying ontology by means of geometric properties of the
embedding space. We analyze complex ontology embedding
methods from four perspectives: embedding method, semantics
complexity, strategy to embed ABox and RBox axioms, and
theoretical analysis; corresponding categorization and related
works are shown in Figure 5.
Multiple geometric models have been developed for the
lightweight DL EL++. ELEmbeddings [22] and EmEL++ [68]
represent named concepts as n-dimensional Euclidean balls,
which cannot faithfully model concept intersection since the
intersection of two balls is no longer a ball in Rn. To ad-
dress this issue, boxes have been adopted [54][69][55]. These
methods can be categorized further by the relation model
implemented within their frameworks: ELBE [54] utilizes
TransE [14]; this model based on vector translations cannot
faithfully represent 1-to-N, N-to-N and N-to-1 relations. To
overcome this limitation, Box2EL [55] adapts the relational
model of BoxE [97], which also allows for capturing role
composition and role subsumption. BoxEL [69] develops an
alternative way to elucidate relations by affine transformations
which solves the problem of concept embedding size incom-
patibility. In [71] a proper non-convex geometric interpretation
for concepts, roles and individuals is introduced first through
mapping of interpretation domain elements into binary vectors,
and then the convex hulls of constructed regions are consid-
ered. This method’s applicability remains an open question
since it lacks implementation and empirical evaluation. [70]
is another method that is mainly described from a theoretical
perspective. It uses axis-aligned cones for concept interpreta-
tions in ALC ontologies. As opposed to previously discussed
works, this method builds partial models: as an example, if
for some individual a only assertion axioms of (C ⊔D)(a)
and C(a) are presented within the ontology and neither
D(a) nor (¬D)(a) can be proven, the embedding leverages
multiple interpretations since there are several dissimilar ways
to interpret a within this geometric framework. There are some
other methods tailored to encode ALC theories, using fuzzy
sets [96] or ordered vector spaces [72].
From the perspective of the expressivity of the DL embed-
ded, many geometric models focus on encoding the constructs
of EL++. ELEmbeddings [22], ELBE [54] and BoxEL [69]
work with the EL fragment enriched with nominals omitting
role inclusions. Subsumption axioms are interpreted as the
containment of one geometric region within another one.
EmEL++ [68] and Box2EL [55] include objective functions for
role inclusion and role chain axioms adopting geometrical con-
tainment for role interpretations: in Box2EL, a box inclusion
loss is used towards boxes that represent the head and tail parts
of a role for role inclusion and chain axioms; in EmEL++ [68],
the role hierarchy is interpreted via establishing partial order
for vectors. [71] provides detailed information about each ele-
ment in the interpretation domain (including to what individual
it corresponds, in which concepts it is contained and how
it is related to other elements); while this approach can be
applied to ALC, it requires to store sparse vectors of size
|NI|+|NC|+|NR|·|∆I|, where ∆I is the interpretation do-
main. Other methods under consideration [70][96][72] target
the full expressivity of ALC; in particular, the negation of a


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
8
Embedding
Complex Ontology
Embedding Method
Euclidean balls [22], [68]
Axis-aligned boxes
TransE [54]
BoxE [55]
Affine transformation [69]
convex/non-convex regions [71]
Axis-aligned cones [70]
Fuzzy sets [96]
Order embedding [72]
Semantics complexity
ELO⊥[22], [54], [69]
ELHO(◦)⊥[68], [55]
ELH [71]
ALC [70], [96], [72]
Ontology Representation
TBox [22], [54], [72]
TBox + ABox [69], [70], [96]
TBox + RBox [68]
ABox + TBox + RBox [71], [55]
Theoretical Analysis
(proofs provided)
Faithfulness [22], [55], [69], [71], [70], [96]
No proofs provided [68], [54], [72]
Fig. 5: Dimensions, their values and corresponding works of embedding complex ontology.
concept is incorporated as a membership function of (¬C)I
assigned to all individuals via fuzzy negation operator applied
to the corresponding membership function of CI [96], by
concept lattice enrichment [72], or using polar operators [70].
An ontology may have an ABox and an RBox, besides a
TBox. Some embedding methods do not distinguish between
individuals and concepts. They eliminate ABox and work
solely with TBox axioms [22][54][72] by translating asser-
tional axioms C(a) and r(a, b) into TBox axioms {a} ⊑C
and {a} ⊑∃r.{b}, respectively. Those methods allowing for
role chain and role inclusion [68][55] incorporate additionally
RBox constructs. In the case that the ABox is retained,
assertional axioms are either embedded alongside with ter-
minological axioms [96][69][55] or placed into the latent
space after the model of TBox is constructed [70]. In contrast
with other approaches, the method in [71] does not explicitly
represent the geometric interpretations of individuals, roles and
concepts for ELH ontologies: it constructs a binary vector
µ(d) of size |NI| + |NC| + |NR| · |∆I| associated with
each domain element d such that µ(d)[a] = 1 if d = aI
(for individuals), µ(d)[A] = 1 if d ∈AI (for classes) and
µ(d)[r, e] = 1 if (d, e) ∈rI (for relations) (µ(d)[a] = 0,
µ(d)[A] = 0, µ(d)[r, e] = 0 otherwise).
In order to show that learned embeddings construct a logical
geometric model of a given ontology, many methods provide
an explicit proof. In most cases, theoretical results state that if
the optimization objective converges to a certain value during
training and some other conditions are satisfied, the theory
has a model [55][22][96][69], and this model is constructed
through the optimization process; in this sense, faithfulness
(machine learning) holds. Sometimes, authors refer to this
property as soundness [55][22][96]. For ELH ontology em-
beddings [71] and axis-aligned cone embeddings [70] our
definition of faithfulness is not applicable since in these
works only interpretation domains and interpretation functions
are introduced without describing optimization process which
approximates geometric models. Definitions of strong and
weak faithfulness discussed in these works are properties of
an interpretation function I.
As for works that do not provide theoretical proofs,
EmEL++ [68] cannot faithfully capture the role hierarchy
although the corresponding loss is present: the r ⊑s loss
is symmetric with respect to r and s, so r ⊑s necessarily
implies s ⊑r; the same holds for role chains r1 ◦r2 ⊑s and
r2 ◦r1 ⊑s. ELBE [54] has faithfulness property which relies
on justifications discussed in [22] since each of its normal
form losses is a multi-dimensional version of corresponding
ELEmbeddings objectives; faithfulness (machine learning) in
this particular case can be formulated as follows (using nota-
tions from the paper): if margin is a vector with non-positive


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
9
Embedding Ontology
with Literals
Embedding Method
Contextual sequence modeling [85][87][88][98][89][90][91]
Non-contextual sequence modeling [81][82][83][24][25][86]
Graph propagation [93][56]
Geometric modeling [26]
Neural-symbolic integration [24][25]
Embedding Type
General encoder [81][82][83][84][24][85][25][87][26][90][91]
Encoder coupled to downstream models [93][86][88][98][89][56]
Ontology Type
Simple ontology [81][82][83][84][85][93][86][87][88][98][90][56][91]
Complex ontology (RDFS/OWL expressions) [24][25][26][89]
Literal Type
Entity names [82][83][84][85][93][86][87][98][89][90][56][91]
Entity names, descriptions and others [81][24][25][26][88]
Fig. 6: Dimensions, their values and corresponding works of embedding ontology with literals.
components and the total loss is equal to 0 then the model is
constructed. CatE [72] does not generate models, yet it can be
considered as faithful with respect to the lattice of ontology
concepts in the sense that the transformation from the ontology
to the lattice is total and injective, and the embedding preserves
lattice structure.
Based on the analysis of these geometric models for com-
plex ontology embedding, we have the following discussions:
• These models adopt highly interpretable balls and axis-
aligned boxes as well as fuzzy sets and order embeddings,
supporting constructs that are not covered by simple ontol-
ogy embedding models [68][55][70][96][71], and contribut-
ing more fine-grained ontology embeddings [69][72][54].
• In terms of expressivity, these models are tailored to main
DL constructs of either EL++ [22][68][54][55][69][71] or
ALC [70][96][72]. As one potential future direction, we can
mention targeting the full expressivity of EL++, motivated
by real-world applications (e.g., many ontologies in biomed-
ical domain) and stronger reasoning capabilities.
• Applicability to real-world scenarios remains an open ques-
tion for some embedding methods [70][71], requiring more
evaluation on real-world ontologies and tasks.
D. Embedding Ontology with Literals
We analyse those works for embedding ontologies with lit-
erals from four dimensions — embedding method, embedding
type, ontology type and literal type. Their potential values
as well as the corresponding works are shown in Figure 6.
For embedding type, we divide the methods into two kinds:
general encoders which are separately trained and applicable
to different downstream tasks, and coupled encoders which
are jointly trained with some other attached models often for
feature learning and cannot be applied without these models.
For literal type, we classify the works into those only use entity
name information, and those use entity name information and
other literals such as long descriptions.
Most methods for embedding ontologies with literals adopt
the solution of sequential modeling demonstrated in Figure 2.
The biggest challenge of this solution lies in its first step —
extracting literal-injected sequences from the ontology. Some
methods such as ERSOM [81], DeepAlignment [83], N-ball
Embeddings [84], SapBERT [87] and HiT [91] directly use
the literals of an entity like names and descriptions as its
sequences to learn its embedding. Such literal-alone sequences
miss the formal semantics of the entity, and therefore, some
other methods such as BERTSubs [89], SORBERT [90] and
[85] extract context-augmented literal sequences by first ex-
ploring the serialization of an entity’s contexts such as the
axioms this entity is involved and its neighbourhood in a graph
transformed from the ontology, and then textualisating the
entity sequences by replacing (a part) of the entities by their
names. For deeper integration of the literals and the formal
semantics, some more complex strategies, such as merging
corpora of different kinds of sequences, and concatenating
a literal-alone sequence and a context-augmented literal se-
quence of an entity for a hybrid literal sequence, have also
been proposed in OPA2Vec [24] and OWL2Vec∗[25].
In the second step of this sequence learning solution, many
methods directly train an encoder from the extracted sequences
by unsupervised learning. ERSOM [81] trains a stacked auto-
encoder which is a neural network with several hidden layers;
DeepAlignment [83], OPA2Vec [24] and OWL2Vec∗[25] di-
rectly train a Word2Vec model; [85] trains a biomedical variant
of BERT. There are also some methods trying to utilize some
external tasks and data for training. SORBERT [90] trains
a sentence transformer with a siamese network architecture
by minimizing the distance between matched concepts from
two ontologies; SapBERT [87] trains a biomedical BERT by
minimizing the distance between a mention from text and its
matched entity in an ontology; HiT [91] re-trains variants
of BERT by retaining the subsumption relationships of an
ontology in a Poincar´e ball.


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
10
The encoders by the above two kinds of methods are
usually quite general, i.e., they are applicable to different tasks
and/or different other ontologies beyond the ones for training,
and their embeddings can be fed to different other models.
On the other hand, some encoders for ontology embedding
are trained in conjunction with some downstream models by
specific tasks. BERTSubs [89] fine-tunes a BERT model and
an attached classifier with concept subsumptions; OntoProtein
[88] trains a BERT model in conjunction with learning the
embeddings of proteins and the Gene Ontology. Such learned
ontology embeddings usually have limited generality, and can
only be applied with another jointly trained model.
Besides sequence learning, OntoZSL [26] embeds an RDFS
ontology with entity names and descriptions by a geometric
modeling method which extends TransE with additional losses
between entity representations and literal representations.
MEDTO [93] and [56] both use Graph Neural Networks to
learn concept features via propagation over concept hierarchies
for minimizing distances between matched concepts. OPA2Vec
[24] and OWL2Vec∗[25] also adopt neural-symbolic integra-
tion by employing OWL reasoners for inferring hidden axioms
for augmenting the sequence extraction.
With the above analysis on the current works, we have the
following discussion for ontology embedding with literals:
• Sequence modeling is the most widely used general solution.
Among these works, using contextual word embedding often
leads to better performance in downstream tasks, but it
requires more future efforts to train general encoders.
• Geometric modeling has been explored only in [26]. How-
ever, it can train embeddings with higher interpretability,
through which we can understand how knowledge are in-
ferred and learn the impact of semantics of literals.
• Current works focus on textual literals such as short phrases
of names and long text of descriptions. Literals of other data
types such as numbers are simply regarded as plain text.
More future efforts are worthy while to deal with literals of
different types especially images for more comprehensive
multi-modal ontology embedding.
E. Embedding Ontology with KG
We analyze those works for embedding ontologies with KGs
from two dimensions — embedding method and ontology
type. Note we consider those works that use the KG for
supporting ontology embedding or jointly embed the KG and
the ontology, but ignore those works that only use ontologies
for supporting KG embedding (e.g., [99][100]). See [32] for a
survey of the latter. As all the works under consideration use
geometric modeling, we make a more fine-grained categoriza-
tion according to how concepts are modeled. Their potential
values and corresponding works are shown in Figure 7.
Some works consider joint embedding of hierarchical con-
cepts and a KG with relational facts [73][75][76][78]. TransC
[73] represents each concept as a sphere and each instance as
a point in the Euclidean space. The concept membership is
then modeled by point inclusion in sphere, and the concept
subsumption is modeled by sphere inclusion. Their losses are
jointly minimized together with the translation loss of TransE
that models the facts. For higher expressivity, some more
complex geometric objects are used. TransEllipsoid [76] and
EIKE [78] both model each concept as a high dimensional
ellipsoid, and TransCuboid [76] models each concept as a high
dimensional box. The ellipsoid and box are both modeled by
one vector for the center and another vector for the boundary
(i.e., offset). TransEllipsoid, TransCuboid and EIKE use simi-
lar losses as TransC for training. OntoEA [75] jointly embeds
two KGs and their ontologies. It uses a point to represent each
concept, and models not only concept subsumption, instance
membership and relational facts, but also instance matching
across KGs and concept disjointness.
The other works consider embedding of RDFS ontologies
with KGs [21][74][77]. JOIE [74] transforms the RDFS ontol-
ogy into an ontology view graph, with each concept modeled
as a node, and each relation’s domain concepts and range
concepts connected by this relation. In training, triples of the
ontology view graph are equally modeled as normal KG triples
by a translation loss, and the instance membership is modeled
by a mapping from the instance to the concept. Concept2Box
[77] extends JOIE by modeling each concept as a box and
by replacing the translation loss with a binary cross entropy
loss. EmbedS [21] models each concept as a sphere and each
relation by two spheres — one for its domain and the other for
its range, and uses distance-based losses. However, EmbedS
has not been evaluated.
With the above analysis, we have the following observations
and perspectives on embedding ontology with KG:
• Although the technical solutions of sequence modeling and
graph propagation have not been exploited in the current
works, through transforming the ontology and the KG into
one graph or directly into sequences, they can be applied.
• The current works adopt typical KG completion benchmarks
for evaluation. More real-world benchmarks and complex
tasks are required to full evaluation. Meanwhile, some
complex ontology embedding methods such as Box2EL [55]
are also applicable, but have not been evaluated.
F. Complexity of Ontology Embedding Methods
We analyze the space and time complexity of ontology
embedding methods in Table II. The notation NC, NR, NI
represents the set of concepts, role and individual names within
an ontology, respectively, and d is the embedding dimension.
Space complexity describes the number of memory units to
store embedding vectors. Most of the methods increase linearly
with d because ontology entities are represented as vectors
in Rd. However, some methods, like the one in [70] exhibit
a quadratic space complexity, O(d2), because role entities
are stored as matrices in Rd×d. Particular cases include [71],
where d = |NC| + |NI| + |NR| · |∆I| and each element in
the domain ∆I is mapped to a binary vector v of length d
and [72], where complexity scales linearly with the number
of operators op in ontology axioms. Operators are symbols
⊓, ⊔, ¬, ∃, ∀found in ontology axioms. Since ALC allows
arbitrarily long axioms, we assume that O(d · op) ≫O(d ·
(|NC| + |NR| + |NI|)) in most cases.
Time complexity O(d) involves linear operations such as
scalar multiplication or element-wise vector operations. In


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
11
Embedding Ontology with KG
Embedding Method
Point-based concept modeling [74][75]
Sphere-based concept modeling [73][21]
Ellipsoid and box-based concept modeling [76][77][78]
Ontology Type
Simple ontology & KG [73][75][76][78]
Complex ontology (RDFS expressions) & KG [21][74][77]
Fig. 7: Dimensions, their values and corresponding works of embedding ontology with KG.
contrast, methods with complexity O(d2) involve quadratic
operations which usually take the form of matrix-vector mul-
tiplications. A particular case is [96], where the complexity is
O(d2·|∆I|) because concept descriptions involving existential
or universal restrictions involve aggregation operations over all
elements in the domain ∆I.
Ontology embeddings with literals are particularly different
as the set of entities include not only concepts, individuals
and roles, but also a potentially large vocabulary from descrip-
tions, labels or external documents. Therefore, the complexity
analysis incorporates the vocabulary size |V | and we assume
that |V | ≥|NC| + |NI| + |NR|. Methods in [24] and
[25] incorporate Word2Vec, whose space and time complexity
are O(d · |V |) and O(c · d · log2(|V |)), respectively, where
c represents the context size in Word2Vec. The complexity
of methods that implement random walks [25][56] include
parameters such as number of walks w and walk length l.
We use the parameter L to represent the number of layers in a
neural networks used in methods such as [81] and [93]. Finally,
we do not include methods that involve tranining/fine-tuning
language models because their large number of parameters and
training time can obscure the complexity analysis.
IV. ONTOLOGY EMBEDDINGS FOR KNOWLEDGE
ENGINEERING AND MACHINE LEARNING
A. Knowledge Engineering
1) Ontology Matching: Given two ontologies O1 and O2,
ontology matching (OM) is to find out entity mappings in form
of (e1, e2), where e1 and e2 are from O1 and O2, respectively,
with an equivalence or subsumption relationship [101]. A good
OM system is expected to have a high Precision for the
discovered mappings and a high Recall towards the ground
truth mappings. Sometimes, some entities in one ontology are
given, and for each of them, an OM system is expected to rank
the entities in the other ontology such that the truly matched
entity is ranked in the first position. In this situation, ranking-
based metrics like MRR (Mean Reciprocal Rank) and Hits@K
(K=1, 5, 10, ...) are often used for evaluation.
Traditional systems mostly use some of the following three
techniques: lexical matching, graph structure matching and
logical reasoning. However, they are limited in several aspects
such text understanding and fusion of different semantics.
Ontology embeddings provide a promising solution to address
these limitations, and thus have been applied for OM by
several recent studies. Meanwhile, OM benchmarks that are
specifically developed for evaluating machine learning-based
OM systems such as Bio-ML [102] provide good contexts for
evaluating ontology embeddings.
Most embedding-based OM methods consider literals due
to their important information. The early methods ERSOM
[81] and DeepAlignment [83] directly calculate the distance
of two concept embeddings, while LogMap-ML [103] and
SORBERT [90] further trains a supervised mapping classifier
that uses concept embeddings as input. Both solutions exploit
the embedded semantics for discovering mappings, but the
improvement over the traditional systems is still limited as the
embeddings are general with no specification to OM. For better
performance, most recent OM methods including MEDTO
[93], [86], BERTMap [98], BERTSubs [89] and [56] jointly
learn task specific embeddings of an ontology with a model
for matching. For example, BERTMap [98] fine-tunes a PLM
for encoding concepts with their names, using synonyms from
the two ontologies and the given mappings in option, while
BERTSubs [89] fine-tunes a PLM for encoding concepts with
their contexts for predicting concept subsumption mappings.
2) Ontology Reasoning:
The embeddings of an ontology
can be used to infer its missing knowledge, among which
different forms of concept subsumptions such as C ⊑D,
C ⊓D ⊑E, C ⊑∃r.D and ∃r.D ⊑C, concept memberships,
property domains and ranges are commonly considered by
many current studies (e.g., [55], [69], [68], [25], [89], [18]
and [74]). In evaluation for concept subsumption inference,
the sub-concept is often given, and a set of candidate concepts
are ranked according to the score of being the super-concept,
where MRR and Hits@K are often adopted for performance
measurement. Note the selection of candidate concepts can be
quite flexible, depending on the benchmarking requirement.
For example, they can be all the named or complex concepts
that exist in the ontology, a particularly selected subset of
them, or some particularly constructed complex concepts. The
evaluation for concept membership inference is similar with
an instance given and a set of candidate concepts ranked.
Meanwhile, there are two settings for inference:
• Prediction. A small part of the axioms are splitted out from
all the declared axioms of the ontology for testing, and
the remaining declared axioms are used for training. The
models are expected to capture more generalizable patterns
for achieving better prediction performance.
• Approximate Deductive Inference. The declared axioms are
used for training, while the entailed axioms are used for
testing. This setting is often used for measuring whether the
ontology embeddings have retained all the formal semantics.


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
12
Method
Space Complexity
Time Complexity
Simple Ontology
Order Embeddings [57]
O(d · |NC|)
O(d)
Poincar´e Embeddings [18]
O(d · |NC|)
O(d)
Hyperbolic Entailment Cones [62]
O(d · |NC|)
O(d)
Box Lattices [19]
O(2d · |NC|)
O(d)
Density Order Embeddings [58]
O(2d · |NC|)
O(d)
Smooth Boxes [59]
O(2d · |NC|)
O(d)
Joint Hierarchies with Boxes [60]
O(4d · |NC|)
O(d)
Gumbel Box Embeddings [61]
O(2d · |NC|)
O(d)
HBE [65]
O(d · (|NC| + |NR|))
O(d2)
HYPON [64]
O(d · |NI|)
O(d2)
HyperExpan[92]
O(d · |NC|)
O(d2)
Complex Ontology
ELEmbeddings [22]
O(d · (|NC| + |NI| + |NR|) + |NC| + |NI|)
O(d)
EmEL++ [68]
O(d · (|NC| + |NI| + |NR|) + |NC| + |NI|)
O(d)
ELBE [54]
O(d · (2|NC| + 2|NI| + |NR|))
O(d)
BoxEL [69]
O(d · (2|NC| + |NI| + 2|NR|))
O(d)
Box2EL [55]
O(d · (3|NC| + 2|NI| + 4|NR|))
O(d)
Convex/Non-convex Regions [71]
O(|∆I| · (|NC| + |NI| + |NR| · |∆I|))
O(|∆I|5)
Al-Cones [70]
O(d · (|NC| + |NI|) + d2 · |NR|)
O(d2)
FALCON [96]
O(d · (|NC| + |NI| + |NIe| + |NR|))
O(d2 · |∆I|)
CatE [72]
O(d · op)
O(d)
Ontology with KGs
TransC [73]
O(d · (|NC| + |NI| + |NR|) + |NC|)
O(d)
EmbedS [21]
O(d · (|NC| + |NI| + 2|NR|) + |NC| + |NR|)
O(d)
JOIE [74]
O(d1 · (|NC| + |NR|) + d2 · (|NI| + |NR|))
O(d2
1 + d1d2)
OntoEA [75]
O(d · (|NC| + |NI| + |NR|))
O(d2)
TransEllipsoid/TransCuboid [76]
O(d · (2|NC| + |NI| + |NR|))
O(d)
Concept2Box [77]
O(d · (2|NC| + 3|NR| + |NI|))
O(d · dBERT )
EIKE [78]
O(d · (2|NC| + |NI|) + |NR|)
O(d2)
Ontology with Literals
ERSOM [81]
O(|V | · (|NC| + |NR| + |NI|))
O(L · d2)
DeepAlignment [83]
O(d · |V |)
O(d)
Category Trees [84]
O(logb(|V |))
O(logb(|V |))
MEDTO [93]
O(d · |NC|)
O(L · d2)
Semantic/Structural Embeddings [56]
O(w · l · |V | + d · |V |)
O(w · l · |V | + c · d · log2(|V |) + d2)
OPA2Vec [24]
O(d · |V |)
O(c · d · log2(|V |))
OWL2Vec* [25]
O(w · l · |V | + d · |V |)
O(w · l · |V | + c · d · log2(|V |))
TABLE II: Time and Space Complexity of Ontology Embedding Methods.
Embeddings learned by different solutions are applied for
ontology reasoning using different paradigms. (i) For embed-
dings by geometric modeling, axioms can usually be inferred
by calculating the geometric relationship of vectors. For the
embeddings that represent concepts by boxes, C ⊑D can be
inferred if the box of C is fully inside the box of D. Otherwise,
a score can also be calculated according to the relative volume
of their overlap. (ii) Embeddings by sequence learning and
graph propagation can be regarded as pre-trained machine
learning features and can be fed into another separately trained
machine learning models like binary classifiers for concept
subsumption prediction (e.g., [25][85]) and unsupervised clus-
ters for concept clustering (e.g., [104]).
3) Discussion: We have the following observations for on-
tology embedding for knowledge engineering. (i) Embeddings
by geometric modeling support interpretable inference, but
often perform worse than embeddings by sequence modeling
with literals incorporated. It is challenging but promising to
incorporate literals in geometric modeling. (ii) Most evaluation
assumes a part of the knowledge to infer (e.g., the sub-concept
of a concept subsumption axiom) are given, but such settings
still require much human support in real-life scenarios. We
need more benchmarks and metrics for supporting end-to-
end evaluation. (iii) The application of ontology embeddings
for knowledge engineering mostly lie in OM and inferring
missing knowledge within ontology. Other tasks such as entity
resolution, query answering, knowledge retrieval and ontology
learning from text can be explored.
B. Knowledge Augmented Machine Learning
Ontologies are able to represent information of machine
learning tasks, datasets and algorithms, and thus ontology
embedding can be a medium to inject domain knowledge
into machine learning training or prediction. One typical
aspect for augmentation is dealing with the sample shortage
problem [105][106]. In this part, we introduce a case study
of using ontology embeddings for zero-shot learning (ZSL)
which typically refers to a machine learning classification
task7 with some or all of its testing labels unseen in training
7In machine learning classification, the output is often called class. To
distinguish it with class in ontology, we call the output classification label or
label in brief.


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
13
[107][27]. The model is expected to have high accuracy on
testing samples of both seen and unseen labels.
Ontology-aware ZSL requires to construct or re-use an
ontology that models the relationships of seen and unseen
classification labels, where each label is often represented as a
concept in the ontology. For example, in animal image classi-
fication, such an ontology could represent animal taxonomies,
visual characteristics, habitats, and so on. With the ontology
and its embeddings, there are mainly two paradigms among
the current studies:
• Mapping-based. In training, this paradigm learns a mapping
function from the training samples to map the vector repre-
sentation (e.g., image features) of the input to the ontology
embedding of the output label. In prediction, the mapping
function is applied to map the test input into an embedding,
and the label (either seen or unseen) whose embedding is
closest to this embedding is regarded as the output. It can
also map the label’s embedding to the input’s vector, or map
both to a common vector. One example work is [108] which
uses EL Embedding [22] to embed an ontology of OWL EL
for modeling labels of animals for zero-shot animal image
classification.
• Generative. This paradigm generates samples for an unseen
label according to its embedding in the ontology, by learning
a conditional generative model like Generative Adversarial
Network (GAN). Thus the ZSL problem is transformed
into a normal supervised learning problem. A representative
work is OntoZSL [26] which embeds literal-aware ontolo-
gies for zero-shot image classification, and zero-shot KG
link prediction with unseen relations.
Applying ontology embedding for machine learning sample
shortage is a promising solution of neural-symbolic integra-
tion, but there is still a shortage of successful systems under
deployment. We think the limitation lies in the representation
of more complex knowledge such as uncertain relationships
(e.g., an image of horse may have a background of grass land,
but not always), as well as the automatic construction of the
ontology for a specific task. The corresponding solutions could
be more flexible multi-modal ontologies with different kinds of
literals and example instances, and more tools for knowledge
integration and automatic ontology construction.
V. ONTOLOGY EMBEDDING DOMAIN APPLICATIONS
A. Life Sciences
In life sciences, the development of successful ontologies
such as the Gene Ontology (GO) [9], SNOMED-CT [40]
and the Human Phenotype Ontology [109] has motivated
the development of methods that incorporate ontologies as a
source of background knowledge. With the rise of machine
learning, ontology embedding has become a common approach
to leverage the ontology. In this part, we review works of
two kinds of life science tasks which widely use ontology
embeddings: (i) protein-protein interaction, gene-disease asso-
ciation and protein/gene function prediction, and (ii) healthcare
predictive analysis with Electronic Health Records (EHR).
Methods for these tasks are usually evaluated as (i) classi-
fication systems with metrics such as Precision and Recall,
(ii) ranking systems with metrics like MRR and Hits@K,
or (iii) predictive systems with metrics like Accuracy@K
(K=5,20,...). In particular cases, methods are evaluated on
problem-specific metrics. For example, protein function pre-
diction uses Fmax which is obtained from Precision and Recall
scores, and Smin which computes the level of uncertainty and
misinformation of the predictions.
The central idea of generating ontology embeddings is to
capture an entity’s latent relationships with other entities. For
example, in protein-protein interaction, whose objective is to
predict if two proteins interact, methods such as [24] and [88]
link protein entities to their corresponding functions in the
Gene Ontology, and thus the generated embeddings of protein
entities encode information about their functions that can be
utilised for interaction prediction. A similar idea is followed
in the gene-disease association problem. For example, in [24],
[119], [120] and [122], genes and diseases are linked to
their corresponding phenotypes in a phenotype ontology, with
the goal of capturing phenotypic-related information in the
embeddings. Other strategies, such as [118], generate only
phenotype embeddings and then compute an embedding for
a gene (or disease) using their associated phenotype embed-
dings. In protein function prediction, embeddings for functions
are obtained from GO, whereas embeddings for proteins are
obtained from protein sequences, to which the use of PLMs
is currently predominant [128][129].
In healthcare, there are various EHR predictive analysis
tasks such as mortality prediction, next-admission diagnosis
prediction or hospital readmission prediction where hierarchi-
cal medical concepts are exploited: an ontology is compiled
into a directed acyclic graph composed of its concepts and
its embedding is combined with the textual information from
EHRs. In most works including [110], [111], [112], [116],
[113], [114], [115] and [121], the graph-based attention mech-
anism is applied to leverage the concept hierarchies; another
approach is described in [63] and [121] where concept hierar-
chies are embedded through hyperbolic embedding. Embedded
medical concepts can then be used for training a Recurrent
Neural Network (RNN) for sequential diagnosis prediction or
mortality prediction. [117] uses a dual RNN with co-attention
and max pooling to fuse medical concept hierarchies of patient
diagnoses and drugs for prescription recommendation. Sev-
eral approaches use embeddings from multiple ontologies or
multiple representations of a single ontology: [113] combines
multi-relational ontologies via a graph attention network for
multi-relational ontology embedding; [115] assigns multiple
embeddings for non-leaf nodes (except for the root) of the
ontology’s directed acyclic graph.
The strategies that have been employed to utilise ontology
embeddings for life science can be summarised as follows:
• Similarity-based strategy: Given the trained embeddings of
a pair of entities, this strategy computes the pair’s score
by either directly calling a similarity function or using a
neural network that is additionally trained for prediction.
One typical embedding method that is often applied in this
strategy is OPA2Vec [24], which is to predict protein-protein
interactions and gene-disease associations. More works that
adopt this strategy include [118], [119] and [120]. Their


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
14
Ontology
embeddings in
life sciences
Embedding Method
Word Embedding [24]
Word Embedding + Graph-based attention [110][111][112][113][114][115][116]
Recurrent Neural Networks [117]
Random Walks + Word Embedding [118][119][120][121]
Knowledge Graph Embedding [122][123][88][119][120]
Graph Neural Networks [124][125]
Euclidean Balls/Boxes [120] [126]
Hyperbolic spaces [63][121]
Partial Orders [127]
Application
Protein-protein interaction [24][88]
Gene-disease association [24][118][119][120][122]
Protein-function prediction [125][88][124][126][127]
EHR predictive analysis [115][63][114][111][112][117][121][113][110][116]
Fig. 8: Categorization of ontology embedding works for life sciences.
embeddings are also based on the sequence learning model
Word2Vec, trained with random walks of the graph extracted
from the ontology.
• Graph-based strategy: This strategy is usually based on
a graph created from one or many ontologies, applying
techniques of KG embedding (KGE), neural networks, hy-
perbolic embedding, graph-based attention and so on. [122],
[123] and [88] frame the problem as link prediction on
the graph, and address it by KGE methods such as TransE
[14] and DistMult [46]. OntoProtein [88] uses the graph
to enhance the training of a Protein Language Model for
better protein embeddings. PO2GO [127] embeds a directed
acyclic graph from GO using a partial-order embedding
method and also adopts an additional neural network for
prediction. Works adopting GNNs either frame the problem
as node classification [124] or use embeddings as partial
input of a prediction module [125]. Graph-based attention
models follow the original GRAM method [110] where the
final embeddings of leaf concepts are convex combinations
of their own embeddings and their ancestors’ embeddings.
More works include learning interaction of hierarchical
embeddings of drug and diagnoses concepts by a dual RNN
co-attention model [117] and utilizing the Poincar´e ball
model [63][121].
• Model-theoretic strategy: Since OWL ontologies are formal
semantics rooted in Description Logics, this strategy aims
to utilize embeddings that are generated for theories. In
this case, the prediction problem is framed as the inference
of axioms with the embeddings. Methods such as [27] for
protein-function prediction and variations of [120] for gene-
disease associations employ a model-theoretic embedding
method ELEmbeddings [22]. [126] further generates multi-
ple embedding models for approximate semantic entailment.
B. Other Applications
Although ontology utilization in other domains is not as
prevalent as in life science applications, partially due to the
absence of well-curated or widely used domain ontologies,
there are some examples of ontology exploitation with embed-
ding, including ontology-aware classifiers for identifying re-
search topics in scholarly articles [130], enhancing intelligent
transportation systems [131], sentiment analysis [132], event
detection [133] and company cointegration prediction [134]. In
these works, ontologies adopted are developed from scratch.
Ontology embedding strategies in these selected studies are
mostly quite simple or straightforward by applying some word
or KG embedding methods, including Word2Vec in [130] and
[131], XLNet for aspect-based sentiment extraction in [132],
IterE [99] and Node2Vec [45] applied to ontology with KG in
[133] and [134]. The work [135] discusses the task-dependent
and task-independent evaluation of Poincar´e disk embeddings
for the GeoNames ontology [136].
VI. MOWL: A MACHINE LEARNING LIBRARY WITH
ONTOLOGY EMBEDDING METHODS
Many ontology embedding works release the implementa-
tions of their methods or applications. However, researchers
often face compatibility issues between different implemen-
tations, spend considerable time adapting code from various
sources, and struggle to ensure fair comparisons between
methods due to differences in implementation. Furthermore,
there is a shortage of easy-to-use softwares that have im-
plemented multiple ontology embedding methods and can
support the implementation of new methods. mOWL8 [37], a
library that provides functions to manipulate ontologies and
8https://github.com/bio-ontology-research-group/mowl


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
15
use ontologies with machine learning, aims to bridge this
gap. mOWL significantly reduces the engineering overhead for
researchers by providing a unified API that standardizes com-
mon operations and allows fair comparison between methods.
It enables researchers to focus on developing novel embedding
approaches rather than dealing with infrastructure challenges.
mOWL provides Python interfaces and can support the fol-
lowing functions: (i) ontology manipulation, where the OWL
API [137] is accessed for ontology creation, manipulation
and reasoning; (ii) ontology transformation, which enables
extracting different graphs (such as concept hierarchies) and
sequences from ontologies; (iii) implementation of ontology
embedding methods that support ontologies of DL EL++
and ALC, covering required components of many methods
described in Section III-A; (iv) common datasets and evalua-
tion modules for axiom prediction and approximate deductive
inference. mOWL includes several workflows following the
modular design patterns for neural-symbolic systems in [138]:
Fig.9 a) and b) encompass methods that transform ontology
axioms and literals into sequences and use NLP methods
to generate embeddings; Fig.9 c) groups the methods that
construct graphs from ontology axioms and leverage graph
propagation methods to generate embeddings; Fig. 9 d) groups
some model theoretic methods, especially the geometric meth-
ods targeting DL EL++. mOWL’s modular design makes it
particularly suitable for both research and practical appli-
cations. Researchers can easily conduct comparative studies
across different embedding methods, while practitioners can
integrate mOWL into their workflows
To demonstrate mOWL, we use two different tasks and two
benchmarks for each task. Both tasks adopt the prediction
setting as described in Section IV-A2, which is to predict new
axioms from the existing ontology background knowledge.
The first task subsumption prediction is to predict axioms are
of the form C ⊑D where C, D are concept names. The two
benchmarks used for subsumption prediction are constructed
from GO and the Food Ontology[25] and included in mOWL.
The second task protein–protein interaction prediction (PPI)
is to determine whether two proteins interact or not based on
their biological functions in the Gene Ontology. This task is
formulated as prediction of axioms pi ⊑∃interacts with.pj,
where pi, pj are instances of proteins. We tested PPIs for
yeast and human organisms. Both the tasks are framed as
ranking problems, with metrics of Mean Rank (MR), Mean
Reciprocal Rank (MRR), Hits@k and AUC of ROC curve.
In Table III, we showcase eight ontology embedding methods
including methods that involve literals (OPA2Vec, OPA2Vec-
NN, OWL2Vec∗), methods that rely on KGE (OWL2Vec∗-
TransE), geometric methods that target DL EL++ (ELEm-
beddings, BoxEL, Box2EL) and methods that target DL ALC
(CatE). Their implementations in mOWL leverages a standard
interface which not only eases the implementation of ontol-
ogy embedding methods, but also enables their manipulation,
analysis and extension.
VII. CHALLENGES AND FUTURE DIRECTIONS
Although many studies have been done, ontology embed-
ding is still a relatively new direction, and there are several
challenges that prevent ontology embedding from having more
real-world applications. We believe more future works are
required in at least the following aspects.
Efficient Geometric Modeling for Complex Ontologies. On
the one hand, quite a few geometric modeling methods in the
Euclidean space have been proposed, but they are mostly lim-
ited to the main features (constructs) of DL EL++ and ALC.
Many other features like the at-least and at-most restrictions
have not been explored, not to mention modeling the complete
formal semantics of an arbitrary OWL ontology. Thus, we
need to extend these methods to support more features that
are used in real-world ontologies, with faithfulness in option.
On the other hand, the main contents of real-world ontologies
are usually the hierarchical concepts, but their modeling in the
Euclidean space (e.g., by high dimensional boxes) is much less
efficient (i.e., requires many more parameters to learn) than
their modeling in some hyperbolic spaces such as Poincar´e
ball in which the distance of points increases exponentially as
they get closer to the boundary [18]. There have been several
studies to explore geometric modeling in hyperbolic spaces for
ontology embedding, such as [18], [92], [62] and [65], but they
mostly only embed concept hierarchies. Geometric modeling
in hyperbolic spaces for some other ontology features deserves
higher attention.
Utilising and Supporting Large Language Models (LLMs).
LLMs like the GPT series and the Llama series have shown
great success in understanding not only natural language but
also images and (semi-)structured data [139][140]. A promis-
ing idea is to embed ontologies, especially those with literals,
using LLMs. There have been some ontology embedding
methods that use encoder-based language models, such as
OWL2Vec∗[25] and OPA2Vec [24] which use Word2Vec, and
HiT [91] which uses BERT-like Transformer-based encoders,
but the generative LLMs are quite different as they adopt
some decoder or encoder-decoder architectures. Therefore,
novel solutions, such as further pre-training and/or instruc-
tion tuning, need to be explored to apply them for ontol-
ogy embedding. Meanwhile, LLMs also suffer from several
problems including hallucination, black-box and shortage of
domain knowledge. Integrating KGs as well as other (semi-
)structured data is widely regarded as a promising solution
[141][49]. How can we use ontology embedding to integrate
ontologies with LLMs? How to support Retrieval Augmented
Generation (RAG) [142] with ontology embedding for incor-
porating domain knowledge and reasoning? Both questions are
worthwhile for future ontology embedding exploration.
Application in Neural-symbolic Integration and Domains.
Currently, ontology embedding is mostly applied to construct
and curate ontologies themselves, and link prediction for
domains like life sciences As a popular semantic technique,
ontology has a high potential for building neural-symbolic
integration [143], utilizing ontology embedding for incorpo-
rating domain knowledge and reasoning capabilities in ma-
chine learning. Although some works have been proposed
for ontology-based zero-shot and few-shot learning [105], we
believe the research in this direction is far from enough, with
many topics like using ontologies for supporting meta learning
(e.g., model selection) and augmenting explanation, not fully


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
16
Fig. 9: High-level representation of four mOWL’s workflows for generating ontology embeddings, following the design patterns
of neural-symbolic integration in [138]. In this representation, a grey box is a neural-symbolic design pattern that consume
data, symbols or models and produce data symbols or models. Data and symbols are depicted with square boxes, models with
hexahedrons and processes (such as training) with round boxes. Workflows a) and b) represent methods that transform the input
ontology into sequences, and then learns the embeddings using NLP methods. These workflows are suitable for many methods
of the sequence modeling solution. Workflow c) represents methods that transform the ontology into a graph and use graph
embedding methods. This workflow is suitable for the graph propagation solution. Workflow d) directly uses an ontology’s
axioms to learn the embeddings, which are suitable methods of geometric modeling, especially model-theoretic methods.
explored. Meanwhile, more domain applications, both in and
out of life sciences, should be considered for exploring the
potential of ontology embeddings. The current life science
applications, which only consider link prediction of several
simple relations and use formal semantics without literals,
are quite simple. Link prediction with more complex target
relations and data of different modalities can be explored using
literal-aware ontology embedding methods. Other tasks, such
as generation for drug discovery and protein design [144] and
natural language inference for clinical trial [145], can also be
explored with ontology embedding.
Benchmarking. This direction still lacks systematic bench-
marking resources. Ontology construction and inference are
straightforward and effective for evaluating different aspects
of ontology embeddings, including how much formal and
informal semantics the embeddings retain. But most of the
current works limit the evaluation to concept subsumption
inference and concept alignment. More complex tasks in
ontology learning, either utilizing or not utilizing external
resources, such as learning complex concept axioms, and in-
serting new concepts, have been rarely considered. Meanwhile,
these aforementioned tasks for neural-symbolic integration and
domain applications can be considered for benchmarking.
VIII. CONCLUSION
This is a comprehensive survey of ontology embedding
which is to represent knowledge of ontologies in vector spaces
with their semantics (partially) retained. It gives formal defini-
tions and properties of ontology embedding, and summarizes
three different technical solutions, i.e., geometric modeling,
sequence modeling and graph propagation, and categorizes the
studies according to not only the methods used but also the
ontologies they aim at (including simple ontology, complex
ontology in OWL or RDFS, ontology with literal and ontology
with KG). Following the method part, the survey also gives
a relatively complete analysis for the application of ontol-
ogy embedding in knowledge engineering, life sciences and
machine learning augmentation, and demonstrates a library
mOWL developed by the co-authors that has implemented
several typical ontology embedding methods and benchmark-
ing resources. In the end, the survey discusses some potential
future directions, including the interesting topics of integrating
ontology embedding with LLMs.
ACKNOWLEDGEMENT
All the authors participated the discussion, paper writing
and proof reading. Olga contributed the review of complex
ontology embedding (III-C), applications in life sciences (V-A)
and other domains (V-B). Fernando contributed the complexity
analysis of embedding methods (III-F), the review of appli-
cations in life sciences (V-A) and other domains (V-B), and
the demonstration of mOWL (VI). Robert co-led the work
and contributed the definitions and properties (II-B4). Yuan
contributed the review of simple ontology embedding (III-B).
Jiaoyan co-led the work and contributed to the other parts.
This work has been funded by the EPSRC projects On-
toEm (EP/Y017706/1), ConCur (EP/V050869/1) and UK
FIRES (EP/S019111/1), the fundings from King Abdullah
University of Science and Technology (KAUST) Office of
Sponsored Research (OSR) under Award No. URF/1/4675-
01-01, URF/1/4697-01-01, URF/1/5041-01-01, REI/1/5659-
01-01, REI/1/5235-01-01, and FCC/1/1976-46-01, and the
SDAIA-KAUST Center of Excellence in Data Science and
Artificial Intelligence (SDAIA-KAUST AI).
REFERENCES
[1] N. Guarino, D. Oberle, and S. Staab, “What is an ontology?” in
Handbook on ontologies, 2009, pp. 1–17.


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
17
Method
MR
MRR
H@3
H@10
H@100
AUC
Prediction of axioms C ⊑D in GO
OPA2Vec
440
0.112
0.112
0.207
0.571
0.990
OPA2Vec-NN
137
0.167
0.177
0.353
0.780
0.997
OWL2Vec*-Sim
141
0.220
0.245
0.428
0.811
0.997
OWL2Vec*-TransE
7446
0.036
0.037
0.065
0.161
0.832
ELEmbeddings
3279
0.041
0.045
0.111
0.330
0.926
BoxEL
6981
0.007
0.003
0.020
0.076
0.842
Box2EL
3940
0.031
0.027
0.088
0.305
0.911
CatE
4548
0.069
0.081
0.216
0.433
0.897
Prediction of axioms C ⊑D in FoodOn
OPA2Vec
2094
0.081
0.082
0.136
0.349
0.926
OPA2Vec-NN
284
0.112
0.109
0.253
0.645
0.990
OWL2Vec*-Sim
433
0.233
0.267
0.442
0.731
0.985
OWL2Vec*-TransE
7909
0.048
0.052
0.078
0.145
0.719
ELEmbeddings
3088
0.081
0.102
0.168
0.293
0.891
BoxEL
3790
0.028
0.035
0.078
0.192
0.866
Box2EL
4655
0.039
0.051
0.118
0.221
0.835
CatE
4542
0.084
0.146
0.197
0.297
0.839
Prediction of PPI (yeast) axioms of the form pi ⊑∃interacts with.pj
OPA2Vec
396
0.061
0.051
0.128
0.543
0.935
OPA2Vec-NN
172
0.144
0.147
0.326
0.777
0.971
OWL2Vec*-Sim
197
0.149
0.154
0.301
0.730
0.967
OWL2Vec*-TransE
219
0.190
0.203
0.402
0.793
0.964
ELEmbeddings
289
0.101
0.094
0.252
0.730
0.952
BoxEL
231
0.037
0.021
0.073
0.551
0.962
Box2EL
188
0.167
0.190
0.435
0.805
0.969
CatE
259
0.043
0.025
0.093
0.563
0.957
Prediction of PPI (human) axioms of the form pi ⊑∃interacts with.pj
OPA2Vec
678
0.080
0.071
0.177
0.594
0.961
OPA2Vec-NN
390
0.136
0.139
0.285
0.692
0.978
OWL2Vec*-Sim
568
0.131
0.140
0.283
0.639
0.967
OWL2Vec*-TransE
477
0.173
0.187
0.357
0.717
0.972
ELEmbeddings
812
0.081
0.075
0.175
0.573
0.953
BoxEL
411
0.038
0.021
0.079
0.564
0.976
Box2EL
564
0.163
0.175
0.336
0.683
0.967
CatE
492
0.059
0.043
0.136
0.629
0.972
TABLE III: Evaluation of ontology embedding methods in subsumption prediction and protein–protein interacion prediction,
with the dataset and models implemented in mOWL.We report filtered metrics that exclude axioms in the training set.
[2] Y. Raimond, T. Scott, S. Oliver, P. Sinclair, and M. Smethurst, “Use
of semantic web technologies on the BBC web sites,” in Linking
Enterprise Data.
Springer, 2010, pp. 263–283.
[3] J. E. Harrison, S. Weber, R. Jakob, and C. G. Chute, “ICD-11: an
international classification of diseases for the twenty-first century,”
BMC Medical Informatics and Decision Making, vol. 21, pp. 1–10,
2021.
[4] X. L. Dong, “Challenges and innovations in building a product knowl-
edge graph,” in ACM SIGKDD, 2018, pp. 2869–2869.
[5] I. Horrocks, “Ontologies and the semantic web,” Communications of
the ACM, vol. 51, no. 12, pp. 58–67, 2008.
[6] J. Hendler, O. Lassila, and T. Berners-Lee, “The semantic web,”
Scientific American, vol. 284, no. 5, pp. 34–43, 2001.
[7] B. McBride, “The resource description framework (RDF) and its
vocabulary description language RDFS,” in Handbook on ontologies,
2004, pp. 51–65.
[8] F. Baader, I. Horrocks, C. Lutz, and U. Sattler, Introduction to descrip-
tion logic.
Cambridge University Press, 2017.
[9] G. O. Consortium, “The gene ontology resource: 20 years and still
going strong,” Nucleic acids research, vol. 47, no. D1, pp. D330–D338,
2019.
[10] D. M. Dooley, E. J. Griffiths, G. S. Gosal, P. L. Buttigieg, R. Hoehndorf,
M. C. Lange, L. M. Schriml, F. S. Brinkman, and W. W. Hsiao,
“Foodon: a harmonized food ontology to increase global food traceabil-
ity, quality control and data integration,” npj Science of Food, vol. 2,
no. 1, p. 23, 2018.
[11] S. Auer, C. Bizer, G. Kobilarov, J. Lehmann, R. Cyganiak, and Z. Ives,
“DBpedia: A nucleus for a web of open data,” in ISWC, 2007, pp. 722–
735.
[12] F. J. Garc´ıa-Pe˜nalvo, J. Garc´ıa, R. Ther´on S´anchez et al., “Analysis of
the OWL ontologies: A survey,” 2011.
[13] T. Mikolov, K. Chen, G. Corrado, and J. Dean, “Efficient estimation of
word representations in vector space,” arXiv preprint arXiv:1301.3781,
2013.
[14] A. Bordes, N. Usunier, A. Garcia-Duran, J. Weston, and O. Yakhnenko,
“Translating embeddings for modeling multi-relational data,” Advances
in Neural Information Processing Systems, vol. 26, 2013.
[15] P. Ristoski and H. Paulheim, “RDF2Vec: RDF graph embeddings for
data mining,” in ISWC, 2016, pp. 498–514.
[16] Q. Wang, Z. Mao, B. Wang, and L. Guo, “Knowledge graph embed-
ding: A survey of approaches and applications,” IEEE Transactions
on Knowledge and Data Engineering, vol. 29, no. 12, pp. 2724–2743,
2017.
[17] X. Chen, S. Jia, and Y. Xiang, “A review: Knowledge reasoning
over knowledge graph,” Expert Systems with Applications, vol. 141,
p. 112948, 2020.
[18] M. Nickel and D. Kiela, “Poincar´e embeddings for learning hierarchical
representations,” NeurIPS, vol. 30, 2017.
[19] L. Vilnis, X. Li, S. Murty, and A. McCallum, “Probabilistic embedding
of knowledge graphs with box lattice measures,” in ACL, 2018, pp.
263–272.
[20] G. A. Miller, “Wordnet: a lexical database for english,” Communica-
tions of the ACM, vol. 38, no. 11, pp. 39–41, 1995.
[21] G. I. Diaz, A. Fokoue, M. Sadoghi et al., “Embeds: Scalable, ontology-
aware graph embeddings.” in EDBT, 2018, pp. 433–436.
[22] M. Kulmanov, W. Liu-Wei, Y. Yan, and R. Hoehndorf, “EL embed-
dings: Geometric construction of models for the description logic
EL++,” in IJCAI, 2019, pp. 6103–6109.


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
18
[23] D. Garg, S. Ikbal, S. K. Srivastava, H. Vishwakarma, H. Karanam,
and L. V. Subramaniam, “Quantum embedding of knowledge for
reasoning,” in NeurIPS, 2019.
[24] F. Z. Smaili, X. Gao, and R. Hoehndorf, “OPA2Vec: combining formal
and informal content of biomedical ontologies to improve similarity-
based prediction,” Bioinformatics, vol. 35, no. 12, pp. 2133–2140,
2019.
[25] J. Chen, P. Hu, E. Jimenez-Ruiz, O. M. Holter, D. Antonyrajah,
and I. Horrocks, “Owl2vec*: Embedding of owl ontologies,” Machine
Learning, vol. 110, no. 7, pp. 1813–1845, 2021.
[26] Y. Geng, J. Chen, Z. Chen, J. Z. Pan, Z. Ye, Z. Yuan, Y. Jia, and
H. Chen, “OntoZSL: Ontology-enhanced zero-shot learning,” in The
Web Conference, 2021, pp. 3325–3336.
[27] M. Kulmanov and R. Hoehndorf, “DeepGOZero: improving protein
function prediction from sequence and zero-shot learning based on
ontology axioms,” Bioinformatics, vol. 38, no. Supplement 1, pp. i238–
i245, 2022.
[28] G. Stoilos, G. B. Stamou, V. Tzouvaras, J. Z. Pan, and I. Horrocks,
“Fuzzy owl: Uncertainty and the semantic web.” in OWLED, vol. 5,
2005, pp. 11–12.
[29] A. Maedche and S. Staab, “Ontology learning for the semantic web,”
IEEE Intelligent systems, vol. 16, no. 2, pp. 72–79, 2001.
[30] J. Lehmann, “Dl-learner: learning concepts in description logics,” The
Journal of Machine Learning Research, vol. 10, pp. 2639–2642, 2009.
[31] S. Badreddine, A. d. Garcez, L. Serafini, and M. Spranger, “Logic
tensor networks,” Artificial Intelligence, vol. 303, p. 103649, 2022.
[32] W. Zhang, J. Chen, J. Li, Z. Xu, J. Z. Pan, and H. Chen, “Knowledge
graph reasoning with logics and embeddings: survey and perspective,”
arXiv preprint arXiv:2202.07412, 2022.
[33] M. Alam, F. van Harmelen, and M. Acosta, “Towards seman-
tically
enriched
embeddings
for
knowledge
graph
completion,”
arXiv:2308.00081, 2023.
[34] B. Xiong, M. Nayyeri, M. Jin, Y. He, M. Cochez, S. Pan, and S. Staab,
“Geometric relational embeddings: A survey,” arXiv:2304.11949, 2023.
[35] M. Kulmanov, F. Z. Smaili, X. Gao, and R. Hoehndorf, “Semantic
similarity and machine learning with ontologies,” Briefings in Bioin-
formatics, vol. 22, no. 4, p. bbaa199, 2021.
[36] F. Z. Smaili, X. Gao, and R. Hoehndorf, “Onto2Vec: joint vector-
based representation of biological entities and their ontology-based
annotations,” Bioinformatics, vol. 34, no. 13, pp. i52–i60, 2018.
[37] F. Zhapa-Camacho, M. Kulmanov, and R. Hoehndorf, “mOWL: Python
library for machine learning with biomedical ontologies,” Bioinformat-
ics, 12 2022.
[38] A. Hogan, E. Blomqvist, M. Cochez, C. d’Amato, G. D. Melo,
C. Gutierrez, S. Kirrane, J. E. L. Gayo, R. Navigli, S. Neumaier et al.,
“Knowledge graphs,” ACM Computing Surveys, vol. 54, no. 4, pp. 1–
37, 2021.
[39] F. Baader, S. Brandt, and C. Lutz, “Pushing the el envelope,” in IJCAI,
2005, pp. 364–369.
[40] K. Donnelly et al., “SNOMED-CT: The advanced terminology and
coding system for ehealth,” Studies in Health Technology and Infor-
matics, vol. 121, p. 279, 2006.
[41] J. Pennington, R. Socher, and C. D. Manning, “Glove: Global vectors
for word representation,” in EMNLP, 2014, pp. 1532–1543.
[42] A. Vaswani, N. Shazeer, N. Parmar, J. Uszkoreit, L. Jones, A. N.
Gomez, Ł. Kaiser, and I. Polosukhin, “Attention is all you need,”
NeurIPS, vol. 30, 2017.
[43] J. D. M.-W. C. Kenton and L. K. Toutanova, “BERT: Pre-training
of deep bidirectional transformers for language understanding,” in
NAACL, 2019, pp. 4171–4186.
[44] E. Asgari and M. R. Mofrad, “Continuous distributed representation
of biological sequences for deep proteomics and genomics,” PloS one,
vol. 10, no. 11, p. e0141287, 2015.
[45] A. Grover and J. Leskovec, “Node2Vec: Scalable feature learning for
networks,” in ACM SIGKDD, 2016, pp. 855–864.
[46] B. Yang, S. W.-t. Yih, X. He, J. Gao, and L. Deng, “Embedding entities
and relations for learning and inference in knowledge bases,” in ICLR,
2015.
[47] T. Dettmers, P. Minervini, P. Stenetorp, and S. Riedel, “Convolutional
2d knowledge graph embeddings,” in AAAI, vol. 32, no. 1, 2018.
[48] G. A. Gesese, R. Biswas, M. Alam, and H. Sack, “A survey on
knowledge graph embeddings with literals: Which model links better
literal-ly?” Semantic Web, vol. 12, no. 4, pp. 617–647, 2021.
[49] J. Z. Pan, S. Razniewski, J.-C. Kalo, S. Singhania, J. Chen, S. Dietze,
H. Jabeen, J. Omeliyanenko, W. Zhang, M. Lissandrini et al., “Large
language models and knowledge graphs: Opportunities and challenges,”
Transactions on Graph Data and Knowledge, pp. 1–38, 2023.
[50] Y. Bengio, A. Courville, and P. Vincent, “Representation learning: A
review and new perspectives,” IEEE Transactions on Pattern Analysis
and Machine Intelligence, vol. 35, no. 8, pp. 1798–1828, 2013.
[51] The Description Logic Handbook: Theory, Implementation and Appli-
cations.
Cambridge University Press, Aug. 2007.
[52] P. Linardatos, V. Papastefanopoulos, and S. Kotsiantis, “Explainable
ai: A review of machine learning interpretability methods,” Entropy,
vol. 23, no. 1, p. 18, 2020.
[53] F. Doshi-Velez and B. Kim, “Towards a rigorous science of inter-
pretable machine learning,” arXiv:1702.08608, 2017.
[54] X. Peng, Z. Tang, M. Kulmanov, K. Niu, and R. Hoehndorf,
“Description logic EL++ embeddings with intersectional closure,”
arXiv:2202.14018, 2022.
[55] M. Jackermeier, J. Chen, and I. Horrocks, “Dual box embeddings for
the description logic el++,” in The Web Conference, 2024.
[56] Z. Hao, W. Mayer, J. Xia, G. Li, L. Qin, and Z. Feng, “Ontology
alignment with semantic and structural embeddings,” Journal of Web
Semantics, vol. 78, p. 100798, 2023.
[57] I. Vendrov, R. Kiros, S. Fidler, and R. Urtasun, “Order-embeddings of
images and language,” in ICML, 2016.
[58] B. Athiwaratkun and A. G. Wilson, “Hierarchical density order em-
beddings,” in ICLR, 2018.
[59] X. Li, L. Vilnis, D. Zhang, M. Boratko, and A. McCallum, “Smoothing
the geometry of probabilistic box embeddings,” in ICLR, 2018.
[60] D. Patel and S. Sankar, “Representing joint hierarchies with box
embeddings,” Automated Knowledge Base Construction, 2020.
[61] S. Dasgupta, M. Boratko, D. Zhang, L. Vilnis, X. Li, and A. McCal-
lum, “Improving local identifiability in probabilistic box embeddings,”
NeurIPS, vol. 33, pp. 182–192, 2020.
[62] O. Ganea, G. B´ecigneul, and T. Hofmann, “Hyperbolic entailment
cones for learning hierarchical embeddings,” in ICML, 2018, pp. 1646–
1655.
[63] Q. Lu, N. De Silva, S. Kafle, J. Cao, D. Dou, T. H. Nguyen, P. Sen,
B. Hailpern, B. Reinwald, and Y. Li, “Learning electronic health
records through hyperbolic embedding of medical ontologies,” in ACM-
BCB, 2019, pp. 338–346.
[64] Z. Li and S. Wang, “HYPON: embedding biomedical ontology with
entity sets,” in ACM-BCB, 2021, pp. 1–7.
[65] Z. Pan and P. Wang, “Hyperbolic hierarchy-aware knowledge graph
embedding for link prediction,” in Findings of the EMNLP, 2021, pp.
2941–2948.
[66] V. Gutierrez Basulto and S. Schockaert, “From knowledge graph
embedding to ontology embedding? an analysis of the compatibility
between vector space representations and rules,” in KR, 2018.
[67]
¨O. L. ¨Ozc¸ep, M. Leemhuis, and D. Wolter, “Cone semantics for logics
with negation,” in IJCAI, 2020, pp. 1820–1826.
[68] S. Mondal, S. Bhatia, and R. Mutharaju, “EmEL++: Embeddings for
εl++ description logic,” in AAAI Spring Symposium on Combining
Machine Learning and Knowledge Engineering, 2021.
[69] B. Xiong, N. Potyka, T.-K. Tran, M. Nayyeri, and S. Staab, “Faithful
embeddings for el++ knowledge bases,” in ISWC, 2022, pp. 22–38.
[70]
¨O. L. ¨Ozcep, M. Leemhuis, and D. Wolter, “Embedding ontologies in
the description logic alc by axis-aligned cones,” Journal of Artificial
Intelligence Research, vol. 78, pp. 217–267, 2023.
[71] V. Lacerda, A. Ozaki, and R. Guimar˜aes, “Strong faithfulness for elh
ontology embeddings,” arXiv:2310.02198, 2023.
[72] F. Zhapa-Camacho and R. Hoehndorf, “Lattice-preserving ALC on-
tology embeddings,” in Neural-Symbolic Learning and Reasoning,
T. R. Besold, A. d’Avila Garcez, E. Jimenez-Ruiz, R. Confalonieri,
P. Madhyastha, and B. Wagner, Eds., 2024, pp. 355–369.
[73] X. Lv, L. Hou, J. Li, and Z. Liu, “Differentiating concepts and instances
for knowledge graph embedding,” in EMNLP, 2018, pp. 1971–1979.
[74] J. Hao, M. Chen, W. Yu, Y. Sun, and W. Wang, “Universal representa-
tion learning of knowledge bases by jointly embedding instances and
ontological concepts,” in ACM SIGKDD, 2019, pp. 1709–1719.
[75] Y. Xiang, Z. Zhang, J. Chen, X. Chen, Z. Lin, and Y. Zheng,
“OntoEA: Ontology-guided entity alignment via joint knowledge graph
embedding,” in Findings of the ACL, 2021, pp. 1117–1128.
[76] J. Yu, C. Zhang, Z. Hu, Y. Ji, D. Fu, and X. Wang, “Geometry-based
anisotropy representation learning of concepts for knowledge graph
embedding,” Applied Intelligence, vol. 53, no. 17, pp. 19 940–19 961,
2023.
[77] Z. Huang, D. Wang, B. Huang, C. Zhang, J. Shang, Y. Liang, Z. Wang,
X. Li, C. Faloutsos, Y. Sun et al., “Concept2Box: Joint geometric
embeddings for learning two-view knowledge graphs,” Findings of the
ACL, 2023.


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
19
[78] K. Wang, G. Qi, J. Chen, and T. Wu, “Embedding ontologies via inco-
prorating extensional and intensional knowledge,” arXiv:2402.01677,
2024.
[79] M. Alshahrani, M. A. Khan, O. Maddouri, A. R. Kinjo, N. Queralt-
Rosinach, and R. Hoehndorf, “Neuro-symbolic representation learning
on biological knowledge graphs,” Bioinformatics, vol. 33, no. 17, pp.
2723–2730, 2017.
[80] O. M. Holter, E. B. Myklebust, J. Chen, and E. Jim´enez-Ruiz,
“Embedding OWL ontologies with OWL2Vec,” in CEUR Workshop
Proceedings, vol. 2456, 2019, pp. 33–36.
[81] C. Xiang, T. Jiang, B. Chang, and Z. Sui, “ERSOM: A structural
ontology matching approach using automatically learned entity rep-
resentation,” in EMNLP, 2015, pp. 2419–2429.
[82] V. Jayawardana, D. Lakmal, N. de Silva, A. S. Perera, K. Sugathadasa,
and B. Ayesha, “Deriving a representative vector for ontology classes
with instance word vector embeddings,” in INTECH, 2017, pp. 79–84.
[83] P. Kolyvakis, A. Kalousis, and D. Kiritsis, “DeepAlignment: Unsuper-
vised ontology matching with refined word vectors,” in NAACL, 2018,
pp. 787–798.
[84] T. Dong, C. Bauckhage, H. Jin, J. Li, O. Cremers, D. Speicher, A. B.
Cremers, and J. Zimmermann, “Imposing category trees onto word-
embeddings using a geometric construction,” in ICLR, 2019.
[85] H. Liu, Y. Perl, and J. Geller, “Concept placement using BERT trained
by transforming and summarizing biomedical ontology structure,”
Journal of Biomedical Informatics, vol. 112, p. 103607, 2020.
[86] V. Nguyen, H. Y. Yip, and O. Bodenreider, “Biomedical vocabulary
alignment at scale in the UMLS metathesaurus,” in The Web Confer-
ence, 2021, pp. 2672–2683.
[87] F. Liu, E. Shareghi, Z. Meng, M. Basaldella, and N. Collier, “Self-
alignment pretraining for biomedical entity representations,” in NAACL,
2021, pp. 4228–4238.
[88] N. Zhang, Z. Bi, X. Liang, S. Cheng, H. Hong, S. Deng, Q. Zhang,
J. Lian, and H. Chen, “OntoProtein: Protein pretraining with gene
ontology embedding,” in ICLR, 2022.
[89] J. Chen, Y. He, Y. Geng, E. Jim´enez-Ruiz, H. Dong, and I. Horrocks,
“Contextual semantic embeddings for ontology subsumption predic-
tion,” World Wide Web, vol. 26, no. 5, pp. 2569–2591, 2023.
[90] F. Gosselin and A. Zouaq, “SORBET: A siamese network for ontology
embeddings using a distance-based regression loss and bert,” in ISWC,
2023, pp. 561–578.
[91] Y. He, Z. Yuan, J. Chen, and I. Horrocks, “Language models as
hierarchy encoders,” arXiv:2401.11374, 2024.
[92] M. D. Ma, M. Chen, T.-L. Wu, and N. Peng, “HyperExpan: Taxonomy
expansion with hyperbolic representation learning,” in Findings of the
EMNLP, 2021, pp. 4182–4194.
[93] J. Hao, C. Lei, V. Efthymiou, A. Quamar, F. ¨Ozcan, Y. Sun, and
W. Wang, “Medto: Medical data to ontology matching using hybrid
graph neural networks,” in ACM SIGKDD, 2021, pp. 2946–2954.
[94] M. Nickel and D. Kiela, “Learning continuous hierarchies in the lorentz
model of hyperbolic geometry,” in ICML, 2018, pp. 3779–3788.
[95] I. Chami, Z. Ying, C. R´e, and J. Leskovec, “Hyperbolic graph convo-
lutional neural networks,” NeurIPS, vol. 32, 2019.
[96] Z. Tang, T. Hinnerichs, X. Peng, X. Zhang, and R. Hoehndorf, “Falcon:
Sound and complete neural semantic entailment over alc ontologies,”
2022.
[97] R. Abboud, I. Ceylan, T. Lukasiewicz, and T. Salvatori, “BoxE: A box
embedding model for knowledge base completion,” NeurIPS, vol. 33,
pp. 9649–9661, 2020.
[98] Y. He, J. Chen, D. Antonyrajah, and I. Horrocks, “BERTMap: a bert-
based ontology alignment system,” in AAAI, vol. 36, no. 5, 2022, pp.
5684–5691.
[99] W. Zhang, B. Paudel, L. Wang, J. Chen, H. Zhu, W. Zhang, A. Bern-
stein, and H. Chen, “Iteratively learning embeddings and rules for
knowledge graph reasoning,” in The World Wide Web Conference, 2019,
pp. 2366–2377.
[100] R. Xie, Z. Liu, M. Sun et al., “Representation learning of knowledge
graphs with hierarchical types.” in IJCAI, vol. 2016, 2016, pp. 2965–
2971.
[101] L. Otero-Cerdeira, F. J. Rodr´ıguez-Mart´ınez, and A. G´omez-Rodr´ıguez,
“Ontology matching: A literature review,” Expert Systems with Appli-
cations, vol. 42, no. 2, pp. 949–971, 2015.
[102] Y. He, J. Chen, H. Dong, E. Jim´enez-Ruiz, A. Hadian, and I. Horrocks,
“Machine learning-friendly biomedical datasets for equivalence and
subsumption ontology matching,” in ISWC, 2022, pp. 575–591.
[103] J. Chen, E. Jim´enez-Ruiz, I. Horrocks, D. Antonyrajah, A. Hadian, and
J. Lee, “Augmenting ontology alignment by semantic embedding and
distant supervision,” in ESWC, 2021, pp. 392–408.
[104] A. Ritchie, J. Chen, L. J. Castro, D. Rebholz-Schuhmann, and
E. Jim´enez-Ruiz, “Ontology clustering with owl2vec,” in CEUR Work-
shop Proceedings, vol. 2918, 2021, pp. 54–61.
[105] J. Chen, Y. Geng, Z. Chen, J. Z. Pan, Y. He, W. Zhang, I. Horrocks,
and H. Chen, “Zero-shot and few-shot learning with knowledge graphs:
A comprehensive survey,” Proceedings of the IEEE, 2023.
[106] J. Chen, Y. Geng, Z. Chen, I. Horrocks, J. Z. Pan, and H. Chen,
“Knowledge-aware zero-shot learning: Survey and perspective,” in
IJCAI, 2021.
[107] Y. Xian, B. Schiele, and Z. Akata, “Zero-shot learning-the good, the
bad and the ugly,” in CVPR, 2017, pp. 4582–4591.
[108] J. Chen, F. L´ecu´e, Y. Geng, J. Z. Pan, and H. Chen, “Ontology-guided
semantic composition for zero-shot learning,” in KR, vol. 17, no. 1,
2020, pp. 850–854.
[109] P. N. Robinson, S. K¨ohler, S. Bauer, D. Seelow, D. Horn, and
S. Mundlos, “The human phenotype ontology: A tool for annotating
and analyzing human hereditary disease,” The American Journal of
Human Genetics, vol. 83, no. 5, p. 610–615, Nov. 2008.
[110] E. Choi, M. T. Bahadori, L. Song, W. F. Stewart, and J. Sun, “Gram:
graph-based attention model for healthcare representation learning,” in
ACM SIGKDD, 2017, pp. 787–795.
[111] X. Peng, G. Long, T. Shen, S. Wang, and J. Jiang, “Sequential diagnosis
prediction with transformer and ontological representation,” in ICDM,
2021, pp. 489–498.
[112] K. Niu, Y. Lu, X. Peng, and J. Zeng, “Fusion of sequential visits
and medical ontology for mortality prediction,” Journal of Biomedical
Informatics, vol. 127, p. 104012, 2022.
[113] C. W. Cheong, K. Yin, W. K. Cheung, B. C. Fung, and J. Poon,
“Adaptive integration of categorical and multi-relational ontologies
with ehr data for medical concept embedding,” ACM Transactions on
Intelligent Systems and Technology, vol. 14, no. 6, pp. 1–20, 2023.
[114] M. Zhang, C. R. King, M. Avidan, and Y. Chen, “Hierarchical attention
propagation for healthcare representation learning,” in ACM SIGKDD,
2020, pp. 249–256.
[115] L. Song, C. W. Cheong, K. Yin, W. K. Cheung, B. C. Fung, and
J. Poon, “Medical concept embedding with multiple ontological repre-
sentations.” in IJCAI, vol. 19, 2019, pp. 4613–4619.
[116] F. Ma, Q. You, H. Xiao, R. Chitta, J. Zhou, and J. Gao, “Kame:
Knowledge-based attention model for diagnosis prediction in health-
care,” in CIKM, 2018, pp. 743–752.
[117] Z. Yao, B. Liu, F. Wang, D. Sow, and Y. Li, “Ontology-aware pre-
scription recommendation in treatment pathways using multi-evidence
healthcare data,” ACM Transactions on Information Systems, vol. 41,
no. 4, pp. 1–29, 2023.
[118] F. Shen, S. Peng, Y. Fan, A. Wen, S. Liu, Y. Wang, L. Wang, and
H. Liu, “Hpo2vec+: Leveraging heterogeneous knowledge resources to
enrich node embeddings for the human phenotype ontology,” Journal
of Biomedical Informatics, vol. 96, p. 103246, 2019.
[119] S. Nunes, R. T. Sousa, and C. Pesquita, “Multi-domain knowledge
graph embeddings for gene-disease association prediction,” Journal of
Biomedical Semantics, vol. 14, no. 1, Aug. 2023.
[120] A. Althagafi, F. Zhapa-Camacho, and R. Hoehndorf, “Prioritizing
genomic variants through neuro-symbolic, knowledge-enhanced learn-
ing,” Bioinformatics, p. btae301, 05 2024.
[121] K. Agarwal, T. Eftimov, R. Addanki, S. Choudhury, S. Tamang, and
R. Rallo, “Snomed2vec: Random walk and poincar\’e embeddings of
a clinical knowledge base for healthcare analytics,” arXiv:1907.08650,
2019.
[122] J. Vilela, M. Asif, A. R. Marques, J. X. Santos, C. Rasga, A. Vicente,
and H. Martiniano, “Biomedical knowledge graph embeddings for
personalized medicine: Predicting disease-gene associations,” Expert
Systems, vol. 40, no. 5, Nov. 2022.
[123] Y. Wang, P. Wegner, D. Domingo-Fern´andez, and A. Tom Ko-
damullil, “Multi-ontology embeddings approach on human-aligned
multi-ontologies representation for gene-disease associations predic-
tion,” Heliyon, vol. 9, no. 11, p. e21502, Nov. 2023.
[124] C. Zhao, T. Liu, and Z. Wang, “PANDA2: protein function prediction
using graph neural networks,” NAR Genomics and Bioinformatics,
vol. 4, no. 1, p. lqac004, 02 2022.
[125] S. Qiu, G. Yu, X. Lu, C. Domeniconi, and M. Guo, “Isoform function
prediction by Gene Ontology embedding,” Bioinformatics, vol. 38,
no. 19, pp. 4581–4588, 08 2022.
[126] M. Kulmanov, F. J. Guzm´an-Vega, P. Duek Roggli, L. Lane, S. T.
Arold, and R. Hoehndorf, “Protein function prediction as approximate
semantic entailment,” Nature Machine Intelligence, vol. 6, no. 2, p.
220–228, Feb. 2024.


ACCEPTED BY IEEE TRANSACTIONS ON KNOWLEDGE AND DATA ENGINEERING (TKDE)
20
[127] W. Li, B. Wang, J. Dai, Y. Kou, X. Chen, Y. Pan, S. Hu, and Z. Z.
Xu, “Partial order relation–based gene ontology embedding improves
protein function prediction,” Briefings in Bioinformatics, vol. 25, no. 2,
p. bbae077, 03 2024.
[128] Z. Lin, H. Akin, R. Rao, B. Hie, Z. Zhu, W. Lu, N. Smetanin,
R. Verkuil, O. Kabeli, Y. Shmueli, A. dos Santos Costa, M. Fazel-
Zarandi, T. Sercu, S. Candido, and A. Rives, “Evolutionary-scale
prediction of atomic-level protein structure with a language model,”
Science, vol. 379, no. 6637, pp. 1123–1130, 2023.
[129] A. Elnaggar, M. Heinzinger, C. Dallago, G. Rehawi, Y. Wang, L. Jones,
T. Gibbs, T. Feher, C. Angerer, M. Steinegger, D. Bhowmik, and
B. Rost, “Prottrans: Toward understanding the language of life through
self-supervised learning,” IEEE Transactions on Pattern Analysis and
Machine Intelligence, vol. 44, no. 10, pp. 7112–7127, 2022.
[130] A. A. Salatino, F. Osborne, T. Thanapalasingam, and E. Motta, “The
cso classifier: Ontology-driven detection of research topics in scholarly
articles,” in TPDL.
Springer, 2019, pp. 296–311.
[131] F. Ali, S. El-Sappagh, and D. Kwak, “Fuzzy ontology and lstm-based
text mining: a transportation network monitoring system for assisting
travel,” Sensors, vol. 19, no. 2, p. 234, 2019.
[132] A. H. Sweidan, N. El-Bendary, and H. Al-Feel, “Sentence-level aspect-
based sentiment analysis for classifying adverse drug reactions (adrs)
using hybrid ontology-xlnet transfer learning,” IEEE Access, vol. 9, pp.
90 828–90 846, 2021.
[133] S. Deng, N. Zhang, L. Li, H. Chen, H. Tou, M. Chen, F. Huang,
and H. Chen, “OntoED: Low-resource event detection with ontology
embedding,” arXiv:2105.10922, 2021.
[134] C. Erten and D. Kazakov, “Ontology graph embeddings and ilp for
financial forecasting,” in ILP.
Springer, 2021, pp. 111–124.
[135] F. Dassereto, L. Di Rocco, G. Guerrini, and M. Bertolotto, “Evaluating
the effectiveness of embeddings in representing the structure of geospa-
tial ontologies,” in The AGILE Conference on Geographic Information
Science.
Springer, 2020, pp. 41–57.
[136] M. Wick, B. Vatant, and B. Christophe, “Geonames ontology,” URL
http://www. geonames. org/ontology, 2015.
[137] M. Horridge and S. Bechhofer, “The OWL API: A Java API for OWL
Ontologies,” Semant. Web, vol. 2, no. 1, p. 11–21, jan 2011.
[138] M. van Bekkum, M. de Boer, F. van Harmelen, A. Meyer-Vitali, and
A. t. Teije, “Modular design patterns for hybrid learning and reasoning
systems: a taxonomy, patterns and use cases,” Applied Intelligence,
vol. 51, no. 9, p. 6528–6546, Jun. 2021.
[139] Y. Chang, X. Wang, J. Wang, Y. Wu, L. Yang, K. Zhu, H. Chen, X. Yi,
C. Wang, Y. Wang et al., “A survey on evaluation of large language
models,” ACM Transactions on Intelligent Systems and Technology,
vol. 15, no. 3, pp. 1–45, 2024.
[140] H. Touvron, T. Lavril, G. Izacard, X. Martinet, M.-A. Lachaux,
T. Lacroix, B. Rozi`ere, N. Goyal, E. Hambro, F. Azhar et al., “Llama:
Open and efficient foundation language models,” arXiv:2302.13971,
2023.
[141] S. Pan, L. Luo, Y. Wang, C. Chen, J. Wang, and X. Wu, “Unifying
large language models and knowledge graphs: A roadmap,” IEEE
Transactions on Knowledge and Data Engineering, 2024.
[142] P. Lewis, E. Perez, A. Piktus, F. Petroni, V. Karpukhin, N. Goyal,
H. K¨uttler, M. Lewis, W.-t. Yih, T. Rockt¨aschel et al., “Retrieval-
augmented generation for knowledge-intensive nlp tasks,” Advances in
Neural Information Processing Systems, vol. 33, pp. 9459–9474, 2020.
[143] P. Hitzler, F. Bianchi, M. Ebrahimi, and M. K. Sarker, “Neural-
symbolic integration and the semantic web,” Semantic Web, vol. 11,
no. 1, pp. 3–11, 2020.
[144] Y. Bian and X.-Q. Xie, “Generative chemistry: drug discovery with
deep learning generative models,” Journal of Molecular Modeling,
vol. 27, pp. 1–18, 2021.
[145] M. Jullien, M. Valentino, H. Frost, P. O’Regan, D. Landers, and
A. Freitas, “Semeval-2023 task 7: Multi-evidence natural language
inference for clinical trial data,” arXiv:2305.02993, 2023.
Jiaoyan Chen is a Lecturer in Department of Com-
puter Science, University of Manchester and a part-
time Senior Researcher in Department of Computer
Science, University of Oxford. His main interests
include KG, ontology and machine learning.
Olga Mashkova is a PhD student in Computer
Science at King Abdullah University of Science and
Technology. Her main interests include deep learn-
ing, bioinformatics and knowledge representation
and reasoning.
Fernando Zhapa-Camacho is a PhD student in
Computer Science at King Abdullah University of
Science and Technology. His main interests include
knowledge representation and reasoning, machine
learning and bioinformatics.
Robert Hoehndorf is an Associate Professor in
Computer Science at King Abdullah University of
Science and Technology. His main interests in-
clude artificial intelligence, knowledge representa-
tion, biomedical informatics and ontology.
Yuan He is a Researcher in Department of Computer
Science, University of Oxford. His main interests
include Large Language Models, knowledge engi-
neering and neural-symbolic integration.
Ian Horrocks is a Professor in Computer Science,
University of Oxford. His main interests include
knowledge representation, ontologies and ontology
languages, description logics, and the Semantic Web.
