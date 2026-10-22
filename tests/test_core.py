import numpy as np

from kikitori import dictionary, pipeline


def test_dictionary_replaces_by_reading():
    r = dictionary.Replacer([("負荷試験", dictionary.norm("ふかしけん"))])
    assert r.apply("来週までに付加試験をやり直します") == "来週までに負荷試験をやり直します"


def test_dictionary_does_not_replace_different_reading():
    r = dictionary.Replacer([("負荷試験", dictionary.norm("ふかしけん"))])
    assert r.apply("この付加価値は高いです") == "この付加価値は高いです"


def test_dictionary_norm_unifies_kana_and_long_vowel():
    assert dictionary.norm("たいぷすくりぷと") == dictionary.norm("タイプスクリプト")
    assert dictionary.norm("コンピューター") == dictionary.norm("コンピュータ")


def test_cluster_merges_to_requested_count():
    v = [np.array([1.0, 0.0]), np.array([0.99, 0.1]), np.array([0.0, 1.0]), np.array([0.1, 0.99])]
    v = [x / np.linalg.norm(x) for x in v]
    labels = pipeline._cluster(v, [1, 1, 1, 1], 2)
    assert labels[0] == labels[1] and labels[2] == labels[3] and labels[0] != labels[2]
